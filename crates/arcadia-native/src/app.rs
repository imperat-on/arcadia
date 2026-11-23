//! Estado da aplicação e arquitetura Elm do iced.
//!
//! `App` guarda somente dados de UI. Trabalho de rede/IPC vira `Task`, e as
//! mensagens voltam para `update` — nenhuma chamada bloqueante roda na thread
//! que desenha.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use arcadia_native_input::NavEvent;
use iced::widget::image::Handle;
use iced::{keyboard, window, Element, Fill, Size, Subscription, Task};
use serde_json::{json, Value};

use crate::backend::{
    AccountProfile, AccountSession, Achievement, AchievementsData, AuthSuccess, Backend,
    CommunityCollectionsPage, CommunityError, CommunityMutation, CommunityReviewsPage,
    DownloadAction, DownloadItem, DownloadsData, EmulationLoad, EmulatorInfo, EmulatorProfile,
    EmulatorRomEntry, EmulatorRomFolder, EmulatorRomImport, EmulatorRomIndexEntry,
    EmulatorScanResult, EmulatorStatus, FriendAchievements, FriendProfile, FriendSearchResult,
    FriendsLoad, Game, GameEmulatorOptions, GameLogOutcome, GameMedia, GameSettingsEnvelope,
    GameSettingsView, GameStats, ImportedEmulatorRom, LaunchEvent, LaunchToggle, NewsItem,
    PackagedPhase, PackagedState, PluginAction, PluginActionOutcome, PluginDetails, PluginItem,
    PluginRegistration, PluginVerification, ProfileStats, ProtonDbInfo, RetroGame, RetroGameDetail,
    RetroOfferDetail, RetroPage, RetroSource, RetroSystemInfo, SettingsConfig, SourceItem, SourceOffers,
    StoreDepot, StoreGame, StoreInfo, StoreInstallInfo, StoreLibrary, StorePage, StoreSearch,
    TrailerResult, UiEvent, UpdateActionResult, UpdateCheck, UpdateCheckReport, UpdateSnapshot,
};
use crate::big_picture::{
    home_shelf, BigPicture, ConsoleEffect, ConsoleTab, ConsoleZone, ACTION_FAVORITE, ACTION_MORE,
    ACTION_PLAY, ACTION_TRAILER,
};
use crate::covers::CoverCache;
use crate::i18n::{I18n, Lang};
use crate::media::{self, MediaPlayer};
use crate::picker::{ArtworkKind, PickPurpose, PickTarget};
use crate::theme;
use crate::toast::ToastQueue;
use crate::views;
use crate::views::game_edit::{GameEditState, MetadataState};
use crate::views::game_menu::{GameMenuState, MenuAction, MenuNotice};
use crate::views::community::{
    CollectionFormState, CommunityCache, CommunityConfirm, CommunityReportState, CommunityState,
    CommunityTab, CommunityTarget, CommunityVisibility, ReviewFormState,
};
use arcadia_native_media::extract_first_visible_frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunityMutationKind {
    ReviewCreate,
    ReviewUpdate,
    ReviewRemove,
    ReviewReport,
    CollectionCreate,
    CollectionUpdate,
    CollectionRemove,
    CollectionReport,
    CollectionItemAdd,
    CollectionItemRemove,
}

impl CommunityMutationKind {
    fn is_review(self) -> bool {
        matches!(
            self,
            Self::ReviewCreate | Self::ReviewUpdate | Self::ReviewRemove | Self::ReviewReport
        )
    }

    fn success_key(self) -> Option<&'static str> {
        match self {
            Self::ReviewCreate | Self::ReviewUpdate => Some("native.community.review_saved"),
            Self::ReviewRemove => Some("native.community.review_removed"),
            Self::CollectionCreate | Self::CollectionUpdate => {
                Some("native.community.collection_saved")
            }
            Self::CollectionRemove => Some("native.community.collection_removed"),
            Self::ReviewReport | Self::CollectionReport => Some("native.community.report_sent"),
            Self::CollectionItemAdd => Some("native.community.item_added"),
            Self::CollectionItemRemove => Some("native.community.item_removed"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    LibraryLoaded(Result<Vec<Game>, String>),
    /// Atualização leve (foco na janela / jogo Steam aguardando instalação):
    /// troca a lista sem spinner e sem derrubar a ficha aberta. `epoch`
    /// descarta respostas de uma leitura que já foi superada.
    LibraryRefreshed {
        epoch: u64,
        result: Result<Vec<Game>, String>,
    },
    /// Poll de 5 s só enquanto a ficha aberta é de jogo Steam não instalado.
    SteamLibraryPoll,
    RefreshVisibleSection,
    /// Abre o menu de contexto com a posição global do cursor.
    OpenGameContextMenu {
        game_id: String,
        anchor: iced::Point,
    },
    CloseGameContextMenu,
    GameMenuAction(MenuAction),
    GameMenuCategoryDraftChanged(String),
    GameMenuCategoryToggled(String),
    GameMenuCategoryAdded,
    GameMenuCategoryBack,
    GameMenuLogsDone {
        game_id: String,
        instance: u64,
        result: Result<GameLogOutcome, String>,
    },
    GameMenuInstallDone {
        game_id: String,
        instance: u64,
        result: Result<(), String>,
    },
    GameMenuKey(keyboard::Event),
    /// Resultado de uma escrita do menu; a lista crua de `overrides:set` é
    /// ignorada e a recarga enriquecida roda na sequência.
    GameOverridesSaved {
        epoch: u64,
        account: Option<String>,
        game_id: String,
        result: Result<(), String>,
    },
    LibraryShowHiddenToggled,
    DismissLibraryNotice,
    GameEditTitleChanged(String),
    GameEditDescriptionChanged(String),
    GameEditDescriptionReset,
    GameEditPlatformChanged(crate::views::game_edit::EditPlatform),
    GameEditExeChanged(String),
    GameEditSave,
    GameEditCancel,
    GameEditSaved {
        game_id: String,
        request: u64,
        account: Option<String>,
        result: Result<(), String>,
    },
    GameMetadataTitleChanged(String),
    GameMetadataDescriptionChanged(String),
    GameMetadataDescriptionReset,
    GameMetadataArtReset(ArtworkKind),
    GameMetadataSave,
    GameMetadataCancel,
    GameMetadataSaved {
        game_id: String,
        request: u64,
        account: Option<String>,
        result: Result<(), String>,
    },
    CoverLoaded {
        url: String,
        loaded: Option<(Handle, (u32, u32), bool)>,
    },
    StoreThumbnailVisible(String),
    StoreThumbnailHidden(String),
    StoreThumbnailLoaded {
        url: String,
        image: Option<(Handle, (u32, u32))>,
    },
    /// O poster de um card decodificou, mas é preto puro (placeholder da
    /// Steam). O vídeo do mesmo card tem a arte real: extrair o primeiro
    /// frame visível e usar no lugar do poster.
    PosterFrameLoaded {
        poster: String,
        frame: Option<(u32, u32, Vec<u8>)>,
    },
    /// Tick dos cards animados: drena os frames dos `<video>` do `about` e
    /// troca a textura de cada card. 16 ms como o tick do trailer.
    CardsTick,
    HomeArtTick,
    Reload,
    SearchChanged(String),
    Navigate(Section),
    OpenGame(String),
    PrefetchGame(String),
    PrefetchStore(StoreGame),
    PrefetchedGame { id: String, media: GameMedia },
    PrefetchedStore { appid: String, result: Result<StoreInfo, String> },
    BackToLibrary,
    /// Resultado de `game:sysinfo` para o jogo aberto no desktop.
    MediaLoaded {
        game_id: String,
        media: GameMedia,
    },
    /// Conquistas Steam/RetroAchievements do jogo aberto.
    AchievementsLoaded {
        game_id: String,
        result: Result<AchievementsData, String>,
    },
    /// ProtonDB do appid do jogo aberto.
    ProtonDbLoaded {
        game_id: String,
        result: Result<Option<ProtonDbInfo>, String>,
    },
    /// Owners/CCU e reviews do jogo aberto.
    GameStatsLoaded {
        game_id: String,
        result: Result<Option<GameStats>, String>,
    },
    /// Miniatura escolhida na faixa de screenshots.
    SelectScreenshot(usize),
    /// Trailer escolhido na galeria (volta do screenshot para o vídeo).
    SelectTrailer,
    /// Aba Mínimos/Recomendados dos requisitos do sistema.
    RequirementsTabSelected(RequirementsTab),
    /// Recarrega `game:sysinfo` e os painéis do jogo atual.
    ReloadGameData,
    /// Resultado de `game:sysinfo` para o painel do Big Picture.
    ConsoleMediaLoaded {
        id: String,
        media: GameMedia,
    },
    /// Resultado de `overrides:set` disparado pelo coração/menu do console.
    ConsoleFavoriteDone {
        id: String,
        result: Result<Vec<Game>, String>,
    },
    /// `news:get` para a aba Notícias do console.
    ConsoleNewsLoaded(Result<Vec<NewsItem>, String>),
    ConsoleNewsSelected(usize),
    ConsoleNewsReload,
    ConsoleMenuAction(usize),
    ConsoleCloseMenu,
    /// Foco programático na busca da Loja (clique ou A no campo).
    ConsoleStoreFocus,
    /// Abre a ficha da loja no console sem sair do Big Picture.
    ConsoleStoreOpenDetail(StoreGame),
    ConsoleStoreCloseDetail,
    ConsoleStoreCategory(usize),
    ConsoleStoreHeroStep(isize),
    ConsoleStoreHeroOpen,
    ConsoleStoreCatalog,
    ConsoleStoreFavorite(String),
    ConsoleStorefrontLoaded { language: Lang, result: Result<crate::backend::StorefrontFeed, String> },
    ConsoleStorefrontRetry,
    /// Tick da Subscription: drena frames e sincroniza o estado do player.
    MediaTick,
    MediaTogglePlayback,
    MediaSeek(f64),
    MediaError(String),
    /// Abre o seletor de trailers do YouTube sobre a ficha do jogo.
    OpenTrailerPicker,
    CloseTrailerPicker,
    TrailerQueryChanged(String),
    TrailerSearchSubmitted,
    TrailerSearchDone {
        query: String,
        result: Result<Vec<TrailerResult>, String>,
    },
    TrailerPathLoaded {
        game_id: String,
        result: Result<Option<String>, String>,
    },
    /// Escolha de um vídeo no seletor; baixa via yt-dlp sem bloquear a UI.
    TrailerDownloadRequested(TrailerResult),
    TrailerDownloadDone {
        game_id: String,
        url: String,
        result: Result<String, String>,
    },
    /// "Abrir trailer baixado" quando `trailer:path` encontrou um arquivo.
    TrailerOpenDownloaded,
    /// "Jogar" na ficha ou no Big Picture: dispara `game:launch`.
    LaunchGame(String),
    /// "Parar": encerra o processo atual via `game:close`.
    StopGame,
    LaunchGameDone {
        id: String,
        result: Result<(), String>,
    },
    StopGameDone(Result<(), String>),
    /// Tick de baixa frequência que drena a fila de eventos do `NativeShell`.
    LaunchEventsTick,
    /// `config:get` silencioso no boot para saber se minimiza ao lançar.
    LaunchConfigLoaded(Result<SettingsConfig, String>),
    /// Bytes de `boot:video` já gravados num arquivo temporário.
    BootVideoReady(Result<Option<PathBuf>, String>),
    /// Tick de ~60 fps do splash de boot.
    BootVideoTick,
    /// Qualquer tecla fecha o splash.
    BootVideoKey(keyboard::Event),
    DismissSplash,
    /// Snapshot de `update:packaged:state` + `update:state` carregado no boot.
    UpdateSnapshotLoaded(Result<UpdateSnapshot, String>),
    /// `true` = checagem manual (abre o diálogo mesmo se já avisado).
    CheckUpdates(bool),
    UpdateCheckDone {
        manual: bool,
        result: Result<UpdateCheckReport, String>,
    },
    UpdateDialogDismiss,
    UpdateBackdropFrame,
    UpdateBackdropCaptured(u64, window::Screenshot),
    UpdateBackdropFailed(u64),
    UpdateBackdropBlurred(u64, Option<Handle>),
    /// Clique bloqueado no backdrop enquanto o updater trabalha.
    UpdateNoop,
    UpdateDownloadRequested,
    UpdateInstallRequested,
    UpdateRetry,
    UpdateActionDone {
        action: UpdateAction,
        result: Result<UpdateActionResult, String>,
    },
    UpdatePollTick,
    UpdateSnapshotPolled(Result<PackagedState, String>),
    UpdateAdvisedDone(Result<(), String>),
    /// Relógio dos toasts de conquista (só roda com a fila não vazia).
    ToastTick,
    DismissToast(u64),
    /// Poll de conquistas a cada 15 s, como o loop do shell Tauri.
    AchievementsPoll,
    AchievementsPollDone,
    /// Abre o diálogo de opções de inicialização do jogo.
    OpenLaunchOptions(String),
    CloseLaunchOptions,
    LaunchOptionsLoaded {
        id: String,
        result: Result<GameSettingsEnvelope, String>,
    },
    LaunchOptionsEmulatorsLoaded {
        id: String,
        result: Result<GameEmulatorOptions, String>,
    },
    LaunchOptionsEmulatorChanged(String),
    LaunchOptionsRomChanged(String),
    LaunchOptionsCoreChanged(String),
    LaunchOptionsEmulatorArgsChanged(String),
    LaunchOptionsValidated {
        id: String,
        patch: Value,
        result: Result<(), String>,
    },
    LaunchOptionsPrefixChanged(String),
    LaunchOptionsExeChanged(String),
    LaunchOptionsArgsChanged(String),
    LaunchOptionsHudChanged(String),
    LaunchOptionsToggle {
        key: LaunchToggle,
        value: bool,
    },
    LaunchOptionsEnvNameChanged(String),
    LaunchOptionsEnvValueChanged(String),
    LaunchOptionsEnvAdd,
    LaunchOptionsEnvRemove(usize),
    /// Abre/fecha os ajustes avançados; é só apresentação, não mexe no rascunho.
    LaunchOptionsAdvancedToggled,
    LaunchOptionsModeChanged(Option<String>),
    LaunchOptionsSave,
    LaunchOptionsSaved {
        id: String,
        result: Result<GameSettingsView, String>,
    },
    Refresh(Section),
    DownloadsLoaded {
        revisions: [u64; 2],
        result: Result<DownloadsData, String>,
    },
    DownloadActionRequested {
        item: DownloadItem,
        action: DownloadAction,
    },
    DownloadActionDone(Result<(), String>),
    SettingsLoaded(Result<SettingsConfig, String>),
    SettingsSaved {
        key: String,
        result: Result<SettingsConfig, String>,
    },
    SettingsTabChanged(SettingsTab),
    AccentChanged(String),
    AccentSave,
    LanguageChanged(String),
    SettingToggled {
        key: &'static str,
        value: bool,
    },
    /// Troca a aba da Biblioteca (todos os jogos / retro).
    LibraryTabSelected(LibraryTab),
    /// Devolve o foco ao trilho do Big Picture (clique num chip de filtro).
    ConsoleFocusRail,
    /// Arte quadrada chegou do canal `arte:quadrada`.
    SquareArtLoaded(Result<std::collections::HashMap<String, String>, String>),
    /// Texto de um token de debrid sendo editado (ainda não salvo).
    DebridTokenChanged {
        provider: DebridProvider,
        value: String,
    },
    /// Salva o token do provedor em `{provider}_token`.
    DebridTokenSave(DebridProvider),
    /// Abre/fecha a linha do provedor.
    DebridToggled(DebridProvider),
    /// Texto do campo SteamGridDB sendo editado (ainda não salvo).
    SteamGridDbKeyChanged(String),
    /// Salva `steamgriddb_api_key`.
    SteamGridDbKeySave,
    /// Conteúdo colado dos cookies do YouTube.
    YoutubeCookiesChanged(String),
    /// Salva `youtube_cookies`.
    YoutubeCookiesSave,
    /// `trailer:downloadAll` — baixa todos os trailers que faltam.
    TrailerDownloadAll,
    TrailerDownloadAllDone(Result<String, String>),
    CpuCoresChanged(i64),
    EmulationLoaded(Result<EmulationLoad, String>),
    EmulationDetect,
    EmulationOpenEditor(String),
    EmulationCloseEditor,
    EmulationExecutableChanged(String),
    EmulationCorePathChanged(String),
    EmulationBiosPathChanged(String),
    EmulationArgsChanged(String),
    EmulationFolderDraftChanged(String),
    EmulationFolderAdd,
    EmulationFolderRemove(usize),
    EmulationFolderRecursiveToggled(usize),
    EmulationProfileSave,
    EmulationProfileSaved(Result<(), String>),
    EmulationRemoveRequest,
    EmulationRemoveCancel,
    EmulationRemoveConfirm,
    EmulationProfileRemoved(Result<(), String>),
    EmulationScan,
    EmulationScanDone(Result<EmulatorScanResult, String>),
    EmulationImportRom {
        path: String,
        name: String,
    },
    EmulationImportDone {
        result: Result<ImportedEmulatorRom, String>,
    },
    SourcesLoaded(Result<Vec<SourceItem>, String>),
    SourceUrlChanged(String),
    SourceAdd,
    SourceRemove(String),
    SourceSync,
    SourcesActionDone(Result<(), String>),
    FriendsLoaded(Result<FriendsLoad, String>),
    ComponentsLoaded(Result<Vec<PluginItem>, String>),
    HubcapKeyChanged(String),
    HubcapKeySave,
    HubcapKeyValidate,
    HubcapKeyChecked {
        ok: bool,
        motivo: Option<String>,
    },
    HubcapStatsLoaded(HubcapStats),
    PluginActionRequested {
        id: String,
        action: PluginAction,
    },
    PluginActionDone {
        id: String,
        action: PluginAction,
        result: Result<PluginActionOutcome, String>,
    },
    PluginVerifyRequested(String),
    PluginVerifyDone {
        id: String,
        result: Result<PluginVerification, String>,
    },
    PluginDetailsRequested(String),
    PluginDetailsDone {
        id: String,
        result: Result<PluginDetails, String>,
    },
    PluginDetailsClose,
    PluginPathChanged(String),
    PluginRegisterRequested,
    PluginRegisterDone(Result<PluginRegistration, String>),
    CommunityTabChanged(CommunityTab),
    CommunityRefresh(CommunityTab),
    CommunityLoadMore(CommunityTab),
    CommunityReviewsLoaded {
        appid: String,
        offset: u64,
        append: bool,
        result: Result<CommunityReviewsPage, CommunityError>,
    },
    CommunityCollectionsLoaded {
        appid: String,
        offset: u64,
        append: bool,
        result: Result<CommunityCollectionsPage, CommunityError>,
    },
    CommunityReviewFormOpened,
    CommunityReviewEditRequested(String),
    CommunityReviewFormClosed,
    CommunityReviewTitleChanged(String),
    CommunityReviewTextChanged(String),
    CommunityReviewRatingChanged(u8),
    CommunityReviewHoursChanged(String),
    CommunityReviewSubmit,
    CommunityCollectionFormOpened,
    CommunityCollectionEditRequested(String),
    CommunityCollectionFormClosed,
    CommunityCollectionTitleChanged(String),
    CommunityCollectionDescriptionChanged(String),
    CommunityCollectionVisibilityChanged(CommunityVisibility),
    CommunityCollectionSubmit,
    CommunityMineToggled(bool),
    CommunityCollectionItemAdd(String),
    CommunityCollectionItemRemove(String),
    CommunityConfirmRequested(CommunityTarget, String),
    CommunityConfirmCancelled,
    CommunityConfirmAccepted,
    CommunityReportRequested(CommunityTarget, String),
    CommunityReportReasonChanged(String),
    CommunityReportSubmit,
    CommunityReportCancelled,
    CommunityMutationDone {
        appid: String,
        action: CommunityMutationKind,
        result: Result<CommunityMutation, CommunityError>,
    },
    StoreTabChanged(StoreTab),
    StoreQueryChanged(String),
    StoreSearchSubmitted,
    StoreClearSearch,
    StoreSuggestionPicked(StoreGame),
    StoreSearchDone {
        query: String,
        result: Result<StoreSearch, String>,
    },
    StoreSuggestDone {
        query: String,
        result: Result<Vec<StoreGame>, String>,
    },
    StoreBrowseLoaded {
        offset: u64,
        generation: u64,
        result: Result<StorePage, String>,
    },
    StorePrefetchNext,
    StoreLoadMore,
    StoreOpenDetail(StoreGame),
    StoreCloseDetail,
    StoreScreenshotSelected(usize),
    StoreSelectTrailer,
    /// Vitrine inicial da Home (`store:recent` top100in2weeks).
    HomeLoaded(Result<StorePage, String>),
    HomeHeroInfoLoaded {
        appid: String,
        result: Result<StoreInfo, String>,
    },
    /// Abre um jogo aleatório da vitrine ("Roll the dice").
    HomeRollDice,
    StoreDetailLoaded {
        appid: String,
        result: Result<StoreInfo, String>,
    },
    StoreInstallInfoLoaded {
        appid: String,
        result: Result<StoreInstallInfo, String>,
    },
    StoreLibrariesLoaded(Result<Vec<StoreLibrary>, String>),
    StoreDepotToggled(String),
    StoreRecommendedDepots,
    StoreClearDepots,
    StoreLibrarySelected(usize),
    StoreInstallRequested,
    StoreSourceOffersLoaded {
        appid: String,
        generation: u64,
        cached_only: bool,
        result: Result<SourceOffers, String>,
    },
    StoreSourcesRetry,
    StoreDownloadRequested,
    StoreDownloadClosed,
    StoreDownloadKey(keyboard::Event),
    StoreSourceSelected(String),
    StoreSourceUriSelected(usize),
    StoreDownloadPathChanged(String),
    StoreSourceDownloadConfirmed,
    StoreSourceDownloadDone {
        appid: String,
        generation: u64,
        result: Result<(), String>,
    },
    /// "Adicionar à biblioteca" da barra da ficha (`store:addToLibrary`).
    StoreAddToLibrary,
    StoreAddedToLibrary {
        appid: String,
        result: Result<Vec<Game>, String>,
        /// Motivo quando a injeção na Steam falhou e o jogo ficou só no Arcadia.
        injection_failed: Option<String>,
    },
    /// "Remover" da barra, quando o jogo já está na biblioteca.
    StoreRemoveFromLibrary,
    StoreRemovedFromLibrary {
        appid: String,
        result: Result<Vec<Game>, String>,
    },
    StoreInstallDone {
        appid: String,
        result: Result<(), String>,
    },
    RetroQueryChanged(String),
    RetroSystemChanged(RetroSystemInfo),
    RetroSearchSubmitted,
    RetroClearSearch,
    RetroPageCached {
        generation: u64,
        offset: u64,
        payload: Value,
        page: Option<RetroPage>,
    },
    RetroPageLoaded {
        generation: u64,
        offset: u64,
        result: Result<RetroPage, String>,
    },
    RetroLibraryArtLoaded {
        id: String,
        art: Option<(String, Option<String>)>,
    },
    RetroPrevPage,
    RetroNextPage,
    RetroOpenDetail(RetroGame),
    RetroCloseDetail,
    RetroDetailLoaded {
        id: String,
        result: Result<RetroGameDetail, String>,
    },
    RetroOfferRequested(String),
    RetroOfferLoaded {
        offer_id: String,
        result: Result<RetroOfferDetail, String>,
    },
    SidebarSearchChanged(String),
    ToggleSidebarGames,
    /// Expande/colapsa o menu da conta na sidebar ("meu perfil").
    ToggleProfileMenu,
    /// Item "Configurações" do menu do perfil: abre Settings na aba Gerais.
    OpenSettingsFromProfile,
    WindowReady(Option<window::Id>),
    WindowDrag,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,
    WindowEvent(window::Event),
    WindowResized(Size),
    /// Entra no modo console (tela cheia + navegação por foco).
    EnterBigPicture,
    /// Volta ao desktop exatamente na tela em que o modo console foi aberto.
    ExitBigPicture,
    /// Tick de ~60 fps: drena o gamepad via `NavInput::tick`.
    ConsoleTick,
    /// Teclado do modo console (setas, Enter, Esc, Tab).
    ConsoleKey(keyboard::Event),
    /// Clique num card do trilho; o segundo clique no card focado abre o painel.
    ConsoleSelectGame(usize),
    /// Clique numa aba da top bar.
    ConsoleSelectTab(ConsoleTab),
    ConsolePlay,
    ConsoleTrailer,
    ConsoleFavorite,
    ConsoleMore,
    ConsoleCloseOverview,
    ConsoleOverviewTab(crate::big_picture::OverviewTab),
    ConsoleOverviewMedia(usize),
    ConsoleOverviewAdvance,
    ConsoleOverviewClosePreview,
    /// `account:status` do boot ou de um reload pós-login/logout.
    AccountStatusLoaded(Result<Option<AccountSession>, String>),
    AccountProfileLoaded(Result<AccountProfile, String>),
    /// `profile:stats` — jogos e horas jogadas do card de Estatísticas.
    ProfileStatsLoaded(Result<ProfileStats, String>),
    /// Quadros do perfil já decodificados fora da thread da interface.
    AnimatedMediaLoaded {
        url: String,
        media: Option<crate::animated_cards::DecodedMedia>,
    },
    /// Recarrega `account:status` + perfil (botão Atualizar do perfil).
    ReloadAccount,
    AccountAvatarLoaded {
        url: String,
        handle: Option<Handle>,
    },
    /// Abre o diálogo de login/cadastro. `required` = pós-logout: Esc/X não
    /// dispensam o diálogo até entrar ou criar conta.
    OpenAuth {
        required: bool,
    },
    AuthClosed,
    AuthModeChanged(AuthMode),
    AuthEmailChanged(String),
    AuthUsernameChanged(String),
    AuthPasswordChanged(String),
    AuthSubmit,
    AuthDone(Result<AuthSuccess, String>),
    /// Enter/Esc do overlay de autenticação.
    AccountKey(keyboard::Event),
    SignOutRequested,
    SignOutDone(Result<(), String>),
    /// Navega até a tela de perfil própria.
    OpenProfile,
    ProfileEditToggled,
    /// Troca a aba do editor (Geral/Avatar/Banner/Fundo).
    ProfileEditTabChanged(ProfileEditTab),
    /// Esc no editor de perfil: volta para o perfil.
    ProfileEditKey(keyboard::Event),
    ProfileFieldChanged {
        field: ProfileField,
        value: String,
    },
    ProfileSave,
    ProfileSaved(Result<(), String>),
    AvatarPathChanged(String),
    AvatarUpload,
    AvatarUploadDone(Result<String, String>),
    /// Limpa o banner/fundo do perfil (patch com `null`).
    ProfileImageCleared(ProfileImageKind),
    ProfileImageClearedDone {
        kind: ProfileImageKind,
        result: Result<(), String>,
    },
    /// Abre o seletor nativo para o alvo indicado.
    PickRequested(PickPurpose),
    /// Resultado do seletor; `None` significa que o usuário cancelou.
    PickDone {
        purpose: PickPurpose,
        result: Result<Option<PathBuf>, String>,
    },
    /// `account:setBackground` concluído para fundo ou banner.
    ProfileImageUploadDone {
        kind: ProfileImageKind,
        result: Result<String, String>,
    },
    FriendSearchChanged(String),
    FriendSearchSubmitted,
    FriendSearchReady(String),
    FriendSearchDone { query: String, result: Result<Vec<FriendSearchResult>, String> },
    FriendMediaLoaded { id: String, url: String, media: Option<crate::animated_cards::DecodedMedia> },
    FriendAvatarLoaded { url: String, media: Option<crate::animated_cards::DecodedMedia> },
    FriendActionRequested {
        id: String,
        action: FriendAction,
    },
    FriendActionDone {
        id: String,
        action: FriendAction,
        result: Result<(), String>,
    },
    FriendProfileRequested(String),
    FriendProfileLoaded {
        id: String,
        result: Result<FriendProfile, String>,
    },
    FriendAchievementsLoaded {
        id: String,
        result: Result<FriendAchievements, String>,
    },
    FriendProfileClosed,
    FriendRemoveConfirmation(bool),
}

/// Ação do updater em voo; define a mensagem de erro/retry do diálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    Download,
    Install,
    ApplyGit,
}

/// Itens fixos da sidebar. Biblioteca, Home e Lojas têm tela real; Big
/// Picture ainda abre um placeholder explícito.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Home,
    Library,
    Stores,
    Components,
    Downloads,
    Sources,
    Friends,
    Settings,
    BigPicture,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Home,
        Section::Library,
        Section::Stores,
        Section::Components,
        Section::Downloads,
        Section::Sources,
        Section::Friends,
        Section::Settings,
    ];

    pub fn label<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            Section::Home => "sidebar.inicio",
            Section::Library => "sidebar.biblioteca",
            Section::Stores => "sidebar.lojas",
            Section::Components => "sidebar.plugins",
            Section::Downloads => "sidebar.downloads",
            Section::Sources => "sidebar.fontes",
            Section::Friends => "amigos.titulo",
            Section::Settings => "settings.title",
            Section::BigPicture => "sidebar.bigpicture",
        };
        i18n.t(key)
    }

    pub fn stub_message<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            Section::Home => "native.section.stub.home",
            Section::Library => "native.section.stub.library",
            Section::Stores => "native.section.stub.stores",
            Section::Components => "native.section.stub.components",
            Section::Downloads => "native.section.stub.downloads",
            Section::Sources => "native.section.stub.sources",
            Section::Friends => "native.section.stub.friends",
            Section::Settings => "native.section.stub.settings",
            Section::BigPicture => "desktop.big_picture_desc",
        };
        i18n.t(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Library,
    Game(String),
    Section(Section),
    /// Perfil da conta logada, acessado pela sidebar (sem item de navegação).
    Profile,
    /// Modo console em tela cheia; o desktop fica guardado em `previous_screen`.
    BigPicture,
}

impl Screen {
    pub fn for_section(section: Section) -> Self {
        if section == Section::Library {
            Self::Library
        } else {
            Self::Section(section)
        }
    }

    pub fn back_to_library(&mut self) {
        *self = Self::Library;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryStatus {
    Loading,
    Ready,
    Error(String),
}

/// Plataformas da Library, na ordem em que aparecem na tela.
///
/// Ordem fixa, não alfabética: Steam primeiro porque é onde está a maior
/// parte da biblioteca, retro depois porque é a coleção que o usuário monta
/// à mão, o resto por último. Reordenar por contagem faria a tela dançar a
/// cada jogo importado.
const PLATFORM_ORDER: [&str; 6] = ["steam", "retro", "epic", "gog", "xbox", "emulador"];

/// Rótulo do grupo de um jogo.
///
/// Devolve `&'static str` para plataforma conhecida e o próprio `launcher`
/// para o resto — o retorno é `Cow` porque `to_lowercase()` cria uma `String`
/// temporária que não pode escapar da função.
fn platform_label(game: &Game) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    if game.is_retro() {
        return Cow::Borrowed("retro");
    }
    match game.launcher.to_lowercase().as_str() {
        "steam" => Cow::Borrowed("steam"),
        "epic" | "epicgames" => Cow::Borrowed("epic"),
        "gog" | "goggalaxy" => Cow::Borrowed("gog"),
        "xbox" | "xcloud" => Cow::Borrowed("xbox"),
        "emulator" | "emulador" | "retroarch" => Cow::Borrowed("emulador"),
        _ => Cow::Borrowed(game.launcher.as_str()),
    }
}

/// Aba da Biblioteca, como o `aba` de `LibraryView.tsx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryTab {
    #[default]
    All,
    Retro,
}

pub struct Library {
    pub games: Vec<Game>,
    pub status: LibraryStatus,
    /// Arte 1:1 por id (`arte:quadrada`). O card do console é quadrado e a
    /// capa da biblioteca é 2:3; esta é a arte que preenche o card sem corte.
    pub square_art: std::collections::HashMap<String, String>,
    /// Aba ativa: separa a biblioteca inteira da retro.
    pub tab: LibraryTab,
    /// Filtro de recuperação: mostra também os ocultos para poder reexibi-los
    /// pelo menu de contexto.
    pub show_hidden: bool,
    /// Aviso de falha de mutação (some ao fechar ou na próxima escrita).
    pub notice: Option<String>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            status: LibraryStatus::Loading,
            square_art: std::collections::HashMap::new(),
            tab: LibraryTab::All,
            show_hidden: false,
            notice: None,
        }
    }
}

impl Library {
    pub fn loading() -> Self {
        Self::default()
    }

    pub fn set_loading(&mut self) {
        self.status = LibraryStatus::Loading;
    }

    pub fn ready(&mut self, games: Vec<Game>) {
        self.games = games;
        self.status = LibraryStatus::Ready;
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.status = LibraryStatus::Error(message.into());
    }

    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|game| game.id == id)
    }

    pub fn game_mut(&mut self, id: &str) -> Option<&mut Game> {
        self.games.iter_mut().find(|game| game.id == id)
    }

    /// Categorias de toda a biblioteca (ocultos inclusive), ordenadas como no
    /// subpainel do `GameContextMenu.tsx`.
    pub fn all_categories(&self) -> Vec<String> {
        let mut categories: BTreeSet<String> = BTreeSet::new();
        for game in &self.games {
            for category in &game.categories {
                let category = category.trim();
                if !category.is_empty() {
                    categories.insert(category.to_string());
                }
            }
        }
        categories.into_iter().collect()
    }

    /// Lista da grade: esconde ocultos, aplica a busca e põe favoritos primeiro.
    pub fn visible_games(&self, query: &str) -> Vec<&Game> {
        let query = query.trim().to_lowercase();
        let mut games: Vec<&Game> = self
            .games
            .iter()
            .filter(|game| self.show_hidden || !game.hidden)
            // A aba separa retro do resto. Sem isto, "Todos os jogos" listava
            // os retro também e a aba Retro só duplicava a mesma grade.
            .filter(|game| match self.tab {
                LibraryTab::Retro => game.is_retro(),
                LibraryTab::All => !game.is_retro(),
            })
            .filter(|game| query.is_empty() || game.title.to_lowercase().contains(query.as_str()))
            .collect();

        games.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        });
        games
    }

    /// Jogos visíveis agrupados por plataforma, como o Orbit separa a Library
    /// em `Steam`, `Retro`, `Xbox`, `Outros`.
    ///
    /// A ordem dos grupos é fixa e não alfabética: as plataformas com mais
    /// jogos vêm primeiro, porque o usuário chega na tela para achar o jogo
    /// que está jogando — não para navegar um índice. Dentro do grupo vale a
    /// ordenação de [`visible_games`] (favorito, depois título).
    ///
    /// Grupo vazio não entra: um cabeçalho "Retro 0" só ocuparia altura.
    pub fn grouped_games(&self, query: &str) -> Vec<(String, Vec<&Game>)> {
        let games = self.visible_games(query);
        let mut groups: Vec<(String, Vec<&Game>)> = Vec::new();

        for label in PLATFORM_ORDER {
            let bucket: Vec<&Game> = games
                .iter()
                .copied()
                .filter(|game| platform_label(game) == *label)
                .collect();
            if !bucket.is_empty() {
                groups.push((label.to_string(), bucket));
            }
        }

        // Plataforma desconhecida não pode sumir da tela: tudo o que não casou
        // com um rótulo conhecido entra num grupo final.
        let known: Vec<&str> = PLATFORM_ORDER.to_vec();
        let rest: Vec<&Game> = games
            .into_iter()
            .filter(|game| {
                let label = platform_label(game);
                !known.contains(&label.as_ref())
            })
            .collect();
        if !rest.is_empty() {
            groups.push((String::new(), rest));
        }

        groups
    }

    /// Lista da sidebar: mesma ordenação, sem o filtro de busca.
    pub fn visible_all(&self) -> Vec<&Game> {
        self.visible_games("")
    }

    /// Todas as artes necessárias para a biblioteca atual, sem repetição.
    pub fn image_urls(&self) -> Vec<String> {
        let mut urls = BTreeSet::new();
        for game in &self.games {
            urls.extend(game.library_portrait_url());
            for url in game.image_urls() {
                urls.insert(url);
            }
        }
        urls.into_iter().collect()
    }
}

/// Escrita de overrides do menu: uma por vez, com geração e conta capturadas
/// para descartar respostas antigas. `dirty` coalesce toggles de categoria
/// feitos enquanto o primeiro `overrides:set` ainda está em voo.
#[derive(Debug, Clone, Default)]
pub struct LibraryOverrideState {
    pub busy: bool,
    pub game_id: Option<String>,
    pub epoch: u64,
    pub account: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameRunStatus {    #[default]
    Idle,
    Launching,
    Running,
    Stopping,
}

/// Sessão de jogo vista pela UI nativa.
///
/// O `LaunchRuntime` do IPC só aceita um jogo por vez; `game_id` diz a qual
/// jogo o estado atual pertence. `error` e `warnings` vêm do dispatcher e são
/// exibidos na ficha e no painel do Big Picture — nada fica só no stderr.
#[derive(Debug, Clone, Default)]
pub struct GameLaunchState {
    pub status: GameRunStatus,
    pub game_id: Option<String>,
    pub error: Option<String>,
    /// Avisos de `game:launchWarning` (ex.: Mangohud não instalado).
    pub warnings: Vec<String>,
    /// Jogo aos quais os avisos pertencem; `None` mostra no jogo aberto.
    pub warnings_game_id: Option<String>,
    /// Preferência `minimize_on_game_launch`, carregada no boot.
    pub minimize_on_launch: bool,
    /// `true` enquanto fomos nós que minimizamos; restaura ao encerrar.
    pub minimized: bool,
}

impl GameLaunchState {
    pub fn is_launching(&self, id: &str) -> bool {
        self.status == GameRunStatus::Launching && self.game_id.as_deref() == Some(id)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.status == GameRunStatus::Running && self.game_id.as_deref() == Some(id)
    }

    pub fn is_stopping(&self, id: &str) -> bool {
        self.status == GameRunStatus::Stopping && self.game_id.as_deref() == Some(id)
    }

    /// Outro jogo está abrindo/rodando; o runtime só aceita uma sessão.
    pub fn busy_with_other(&self, id: &str) -> bool {
        self.status != GameRunStatus::Idle && self.game_id.as_deref() != Some(id)
    }

    /// Avisos do lançamento para o jogo da ficha (vazio se forem de outro).
    pub fn warnings_for(&self, id: &str) -> &[String] {
        match self.warnings_game_id.as_deref() {
            Some(warning_id) if warning_id != id => &[],
            _ => &self.warnings,
        }
    }

    /// Aplica um evento do `LaunchRuntime` sem consultar o processo.
    fn apply_event(&mut self, event: LaunchEvent, i18n: &I18n) {
        match event {
            LaunchEvent::Running(running) => {
                if running {
                    self.status = GameRunStatus::Running;
                    self.error = None;
                } else if !matches!(self.status, GameRunStatus::Launching | GameRunStatus::Idle) {
                    // Durante "Abrindo…" o `idle` do launchState é a fonte de
                    // verdade; um `game:running(false)` atrasado pode ser de
                    // uma sessão anterior.
                    self.status = GameRunStatus::Idle;
                    self.game_id = None;
                }
            }
            LaunchEvent::Active { running, game_id } => {
                if running {
                    self.status = GameRunStatus::Running;
                    if game_id.is_some() {
                        self.game_id = game_id;
                    }
                    self.error = None;
                } else if self.matches_game(game_id.as_deref())
                    && !matches!(self.status, GameRunStatus::Launching | GameRunStatus::Idle)
                {
                    self.status = GameRunStatus::Idle;
                    self.game_id = None;
                }
            }
            LaunchEvent::Phase { phase, game_id } => match phase {
                crate::backend::LaunchPhase::Starting => {
                    self.status = GameRunStatus::Launching;
                    if game_id.is_some() {
                        self.game_id = game_id;
                    }
                    self.error = None;
                    self.warnings.clear();
                    self.warnings_game_id = None;
                }
                crate::backend::LaunchPhase::Running => {
                    if self.matches_game(game_id.as_deref()) {
                        self.status = GameRunStatus::Running;
                        if game_id.is_some() {
                            self.game_id = game_id;
                        }
                        self.error = None;
                    }
                }
                crate::backend::LaunchPhase::Stopping => {
                    if self.matches_game(game_id.as_deref()) {
                        self.status = GameRunStatus::Stopping;
                        if game_id.is_some() {
                            self.game_id = game_id;
                        }
                    }
                }
                crate::backend::LaunchPhase::Idle => {
                    if self.matches_game(game_id.as_deref()) {
                        self.status = GameRunStatus::Idle;
                        self.game_id = None;
                    }
                }
            },
            LaunchEvent::Warning { game_id, warnings } => {
                self.warnings = warnings;
                self.warnings_game_id = game_id;
            }
            LaunchEvent::Error { game_id, error } => {
                if self.matches_game(game_id.as_deref()) {
                    self.status = GameRunStatus::Idle;
                    self.game_id = game_id;
                    self.error =
                        Some(i18n.t_args("native.game.launch_failed", &[("error", &error)]));
                }
            }
        }
    }

    /// Evento sem `gameId` é aceito; com id, só altera a sessão atual.
    fn matches_game(&self, game_id: Option<&str>) -> bool {
        match (game_id, self.game_id.as_deref()) {
            (Some(event_id), Some(current)) => event_id == current,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LoadStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error(String),
}

/// Aba ativa do painel de requisitos da ficha (loja e biblioteca).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RequirementsTab {
    #[default]
    Minimum,
    Recommended,
}

/// Chave do cache de painéis da ficha aberta pela loja. O prefixo evita
/// colidir com o id da biblioteca (`steam:<appid>`).
pub(crate) fn store_panels_id(game: &StoreGame) -> String {
    format!("store:{}", game.appid.trim())
}

/// Estado do diálogo "Opções de inicialização".
///
/// `mode` é escolha de sessão (não existe em `game_settings.json`): vale para
/// os próximos `game:launch` do jogo enquanto ele estiver aberto no diálogo.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptionsState {
    pub open: bool,
    pub game_id: String,
    pub game_title: String,
    pub status: LoadStatus,
    pub settings: Option<GameSettingsView>,
    pub default_prefix: String,
    pub mode: Option<String>,
    /// Ajustes avançados visíveis. Estado de apresentação: começa fechado e
    /// não participa do rascunho nem do `game_settings.json`.
    pub advanced_open: bool,
    pub env_name: String,
    pub env_value: String,
    /// Texto do campo de argumentos do emulador (preserva espaços digitados).
    pub emulator_args_text: String,
    /// Catálogo para o seletor "Nenhum / emulador"; carregado com a ficha.
    pub emulators: Vec<EmulatorInfo>,
    pub emulator_statuses: Vec<EmulatorStatus>,
    pub emulators_error: Option<String>,
    pub saving: bool,
    pub saved: bool,
    /// Há edições no formulário que ainda não foram gravadas.
    pub dirty: bool,
    pub error: Option<String>,
    /// Seletor em voo (ROM/core da seção Emulador).
    pub pick_busy: Option<PickPurpose>,
}

impl LaunchOptionsState {
    /// `mode` de `game:launch` para este jogo; `None` = automático.
    ///
    /// Só vale depois que as edições foram salvas: o `LaunchRuntime` lê o
    /// `game_settings.json`, não o rascunho da UI.
    pub fn mode_for(&self, id: &str) -> Option<&str> {
        if self.game_id == id && !self.dirty && !self.saving {
            self.mode.as_deref()
        } else {
            None
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.saved = false;
    }

    /// Alterna a visibilidade dos ajustes avançados sem tocar em `settings`,
    /// `dirty` ou `saved`: recolher não pode descartar edições.
    pub fn toggle_advanced(&mut self) {
        self.advanced_open = !self.advanced_open;
    }

    /// Aplica ROM/core escolhidos no seletor à seção Emulador. Só aceita o
    /// jogo/emulador atuais para um resultado atrasado não preencher outro.
    pub fn apply_pick(&mut self, purpose: &PickPurpose, path: PathBuf) -> bool {
        if purpose.game_id() != Some(self.game_id.as_str()) {
            return false;
        }
        let Some(settings) = self.settings.as_mut() else {
            return false;
        };
        if purpose.emulator_id() != settings.emulator_id.as_deref() {
            return false;
        }
        let path = path.to_string_lossy().into_owned();
        match purpose.target() {
            PickTarget::GameRom => settings.rom_path = path,
            PickTarget::GameCore => settings.emulator_core_path = path,
            _ => return false,
        }
        self.mark_dirty();
        true
    }

    /// Emulador selecionado no catálogo, se ainda existir.
    pub fn selected_emulator(&self) -> Option<&EmulatorInfo> {
        let id = self.settings.as_ref()?.emulator_id.as_deref()?;
        self.emulators.iter().find(|item| item.id == id)
    }

    /// Status de BIOS/firmware do emulador selecionado.
    pub fn selected_emulator_status(&self) -> Option<&EmulatorStatus> {
        let id = self.settings.as_ref()?.emulator_id.as_deref()?;
        self.emulator_statuses
            .iter()
            .find(|status| status.emulator_id == id)
    }
}

/// Seletor de trailers do YouTube aberto sobre a ficha do jogo.
#[derive(Debug, Clone, Default)]
pub struct TrailerPickerState {
    pub open: bool,
    pub game_id: String,
    pub game_title: String,
    pub query: String,
    pub results: Vec<TrailerResult>,
    pub status: LoadStatus,
    /// `trailer:path` do jogo; `Some(file://…)` quando já existe download.
    pub existing_path: Option<String>,
    /// Vídeo em download (chamada longa do yt-dlp); a UI mostra "Baixando…".
    pub downloading: Option<TrailerResult>,
    pub download_error: Option<String>,
}

/// Aba Notícias do Big Picture: feed `news:get` + erro honesto da consulta.
#[derive(Debug, Clone, Default)]
pub struct NewsState {
    pub items: Vec<NewsItem>,
    pub status: LoadStatus,
    pub error: Option<String>,
}

/// Painel de conquistas da ficha: lista + estado explícito de carga.
#[derive(Debug, Clone, Default)]
pub struct AchievementsPanelState {
    pub status: LoadStatus,
    pub items: Vec<Achievement>,
    /// Ausência explicada (jogo não encontrado no RA, launcher sem suporte).
    pub note: Option<String>,
}

/// Painel ProtonDB da ficha.
#[derive(Debug, Clone, Default)]
pub struct ProtonPanelState {
    pub status: LoadStatus,
    pub info: Option<ProtonDbInfo>,
    /// Ausência explicada (jogo não-Steam, sem relatórios).
    pub note: Option<String>,
}

/// Painel de estatísticas/reviews da ficha.
#[derive(Debug, Clone, Default)]
pub struct StatsPanelState {
    pub status: LoadStatus,
    pub stats: Option<GameStats>,
    /// Ausência explicada (jogo não-Steam, API sem dados).
    pub note: Option<String>,
}

/// Dados dos painéis por jogo; o mapa em [`App`] é o cache da sessão.
#[derive(Debug, Clone, Default)]
pub struct GamePanels {
    pub achievements: AchievementsPanelState,
    pub proton: ProtonPanelState,
    pub stats: StatsPanelState,
}

/// Provedores de debrid, na ordem de tentativa do backend
/// (Real-Debrid → TorBox → AllDebrid → Premiumize).
///
/// A ordem e as chaves de config vêm do catálogo estático do
/// `SettingsPanel.tsx`; manter a mesma ordem aqui é requisito, não estética —
/// o backend tenta nessa sequência.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebridProvider {
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
}

impl DebridProvider {
    pub const ALL: [DebridProvider; 4] = [
        DebridProvider::RealDebrid,
        DebridProvider::TorBox,
        DebridProvider::AllDebrid,
        DebridProvider::Premiumize,
    ];

    /// Nome comercial, como aparece na web.
    pub fn name(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "Real-Debrid",
            DebridProvider::TorBox => "TorBox",
            DebridProvider::AllDebrid => "AllDebrid",
            DebridProvider::Premiumize => "Premiumize",
        }
    }

    /// Chave da config (`{provider}_token`), a mesma que a allowlist do IPC
    /// aceita em `config:set`.
    pub fn config_key(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "realdebrid_token",
            DebridProvider::TorBox => "torbox_token",
            DebridProvider::AllDebrid => "alldebrid_token",
            DebridProvider::Premiumize => "premiumize_token",
        }
    }

    /// Chave i18n da descrição do provedor.
    pub fn description_key(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "settings.realdebrid.desc",
            DebridProvider::TorBox => "settings.torbox.desc",
            DebridProvider::AllDebrid => "settings.alldebrid.desc",
            DebridProvider::Premiumize => "settings.premiumize.desc",
        }
    }
}

/// Aba interna das Configurações, equivalente à sub-navegação da UI web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Integrations,
    Metadata,
    Accessibility,
    Controller,
    Emulation,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 7] = [
        SettingsTab::General,
        SettingsTab::Integrations,
        SettingsTab::Metadata,
        SettingsTab::Accessibility,
        SettingsTab::Controller,
        SettingsTab::Emulation,
        SettingsTab::About,
    ];

    pub fn label<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            SettingsTab::General => "settings.general",
            SettingsTab::Integrations => "settings.integracoes",
            SettingsTab::Metadata => "settings.metadados.titulo",
            SettingsTab::Accessibility => "sidebar.acessibilidade",
            SettingsTab::Controller => "controller.titulo",
            SettingsTab::Emulation => "settings.emulacao",
            SettingsTab::About => "about.titulo",
        };
        i18n.t(key)
    }

    /// `true` só para Emulação: é a única seção que ganha o cabeçalho de grupo
    /// `CLASSICS` na sub-navegação da sidebar, como no `Sidebar.tsx`.
    pub fn starts_group(self) -> bool {
        self == SettingsTab::Emulation
    }
}

#[derive(Debug, Clone, Default)]
pub struct DownloadsState {
    pub data: DownloadsData,
    pub status: LoadStatus,
    pub busy_id: Option<String>,
    pub action_error: Option<String>,
    /// Versões das filas dm e torrent/HTTP. Leituras em voo não podem restaurar
    /// progresso ou itens anteriores a um evento de pausa/cancelamento.
    revisions: [u64; 2],
}

impl DownloadsState {
    fn apply_progress(&mut self, is_torrent: bool, items: Vec<DownloadItem>) {
        let source = usize::from(is_torrent);
        self.revisions[source] = self.revisions[source].wrapping_add(1);
        self.data.items.retain(|item| item.is_torrent != is_torrent);
        self.data.items.extend(items);
        self.data.items.sort_by_key(|item| item.status.rank());
        self.status = LoadStatus::Ready;
    }

    fn apply_snapshot(&mut self, revisions: [u64; 2], mut data: DownloadsData) {
        for is_torrent in [false, true] {
            let source = usize::from(is_torrent);
            if revisions[source] != self.revisions[source] {
                data.items.retain(|item| item.is_torrent != is_torrent);
                data.items.extend(self.data.items.iter()
                    .filter(|item| item.is_torrent == is_torrent).cloned());
            }
        }
        data.items.sort_by_key(|item| item.status.rank());
        self.data = data;
        self.status = LoadStatus::Ready;
    }
}

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub config: Option<SettingsConfig>,
    pub status: LoadStatus,
    pub tab: SettingsTab,
    pub accent_draft: String,
    pub save_error: Option<String>,
    pub saving: Option<String>,
    /// Editor de tokens de debrid: rascunho por provedor e quais linhas estão
    /// abertas. Fica no estado (não na view) porque a view é reconstruída a
    /// cada mensagem e o texto digitado se perderia.
    pub debrid_drafts: DebridDrafts,
    pub debrid_open: std::collections::HashSet<DebridProvider>,
    /// Provedor cujo token acabou de salvar (mostra "Salvo!" por um instante).
    pub debrid_saved: Option<DebridProvider>,
    /// Rascunho da chave do SteamGridDB (aba Metadados).
    pub steamgriddb_draft: String,
    /// `true` logo depois de salvar a chave, para o botão dizer "Salvo!".
    pub steamgriddb_saved: bool,
    /// Rascunho dos cookies do YouTube, colados pelo usuário.
    pub youtube_cookies_draft: String,
    pub youtube_cookies_saved: bool,
    /// Último resultado de `trailer:downloadAll` (mensagem de progresso).
    pub trailer_download_all: Option<String>,
}

/// Rascunho do token de cada provedor, preenchido a partir da config.
#[derive(Debug, Clone, Default)]
pub struct DebridDrafts {
    pub realdebrid: String,
    pub torbox: String,
    pub alldebrid: String,
    pub premiumize: String,
}

impl DebridDrafts {
    pub fn get(&self, provider: DebridProvider) -> &str {
        match provider {
            DebridProvider::RealDebrid => &self.realdebrid,
            DebridProvider::TorBox => &self.torbox,
            DebridProvider::AllDebrid => &self.alldebrid,
            DebridProvider::Premiumize => &self.premiumize,
        }
    }

    pub fn set(&mut self, provider: DebridProvider, value: String) {
        match provider {
            DebridProvider::RealDebrid => self.realdebrid = value,
            DebridProvider::TorBox => self.torbox = value,
            DebridProvider::AllDebrid => self.alldebrid = value,
            DebridProvider::Premiumize => self.premiumize = value,
        }
    }

    /// Preenche a partir da config carregada, sem apagar rascunho em edição.
    pub fn sync_from(&mut self, config: &SettingsConfig) {
        self.realdebrid = config.realdebrid_token.clone();
        self.torbox = config.torbox_token.clone();
        self.alldebrid = config.alldebrid_token.clone();
        self.premiumize = config.premiumize_token.clone();
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            config: None,
            status: LoadStatus::Idle,
            tab: SettingsTab::General,
            accent_draft: String::new(),
            save_error: None,
            saving: None,
            debrid_drafts: DebridDrafts::default(),
            debrid_open: std::collections::HashSet::new(),
            debrid_saved: None,
            steamgriddb_draft: String::new(),
            steamgriddb_saved: false,
            youtube_cookies_draft: String::new(),
            youtube_cookies_saved: false,
            trailer_download_all: None,
        }
    }
}

/// Dados da aba Configurações > Emulação. O estado do editor fica aqui para a
/// view ser somente leitura; nenhuma chamada de IPC sai do desenho.
#[derive(Debug, Clone, Default)]
pub struct EmulationState {
    pub status: LoadStatus,
    pub emulators: Vec<EmulatorInfo>,
    pub statuses: Vec<EmulatorStatus>,
    pub rom_index: BTreeMap<String, EmulatorRomIndexEntry>,
    /// Falhas parciais de status/índice/perfis; o catálogo continua visível.
    pub warnings: Vec<String>,
    pub editor: Option<EmulationEditorState>,
}

impl EmulationState {
    pub fn status_for(&self, id: &str) -> Option<&EmulatorStatus> {
        self.statuses.iter().find(|status| status.emulator_id == id)
    }

    pub fn rom_index_for(&self, id: &str) -> Option<&EmulatorRomIndexEntry> {
        self.rom_index.get(id)
    }
}

/// Rascunho do perfil aberto. `original` guarda o perfil persistido para
/// preservar core/BIOS quando o formulário não os mostra.
#[derive(Debug, Clone)]
pub struct EmulationEditorState {
    pub emulator_id: String,
    pub executable: String,
    pub core_path: String,
    pub bios_path: String,
    pub args_text: String,
    pub rom_folders: Vec<EmulatorRomFolder>,
    pub folder_draft: String,
    pub original: Option<EmulatorProfile>,
    pub requires_core: bool,
    pub bios_required: bool,
    pub saving: bool,
    pub confirm_remove: bool,
    pub error: Option<String>,
    pub scanning: bool,
    pub scan_error: Option<String>,
    pub preview_available: bool,
    pub preview_roms: Vec<EmulatorRomEntry>,
    pub preview_truncated: bool,
    /// ROM cuja importação está em voo (path).
    pub importing_rom: Option<String>,
    pub notice: Option<String>,
    /// Seletor em voo; só um diálogo por vez.
    pub pick_busy: Option<PickPurpose>,
}

/// Falhas ao inserir uma pasta de ROM; a view traduz para o i18n.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomFolderAddError {
    Empty,
    Limit,
    Duplicate,
}

impl EmulationEditorState {
    pub fn add_rom_folder(&mut self, path: &str) -> Result<(), RomFolderAddError> {
        let path = path.trim().to_string();
        if path.is_empty() {
            return Err(RomFolderAddError::Empty);
        }
        if self.rom_folders.len() >= crate::backend::MAX_EMULATOR_ROM_FOLDERS {
            return Err(RomFolderAddError::Limit);
        }
        if self.rom_folders.iter().any(|folder| folder.path == path) {
            return Err(RomFolderAddError::Duplicate);
        }
        self.rom_folders.push(EmulatorRomFolder {
            path,
            recursive: true,
        });
        Ok(())
    }

    /// Aplica o resultado do seletor ao campo correspondente. Devolve `false`
    /// quando o alvo não pertence a este editor/estado (resultado atrasado).
    pub fn apply_pick(
        &mut self,
        purpose: &PickPurpose,
        path: PathBuf,
    ) -> Result<bool, RomFolderAddError> {
        if purpose.emulator_id() != Some(self.emulator_id.as_str()) {
            return Ok(false);
        }
        let path = path.to_string_lossy().into_owned();
        match purpose.target() {
            PickTarget::EmulatorExecutable => self.executable = path,
            PickTarget::EmulatorCore => self.core_path = path,
            PickTarget::EmulatorBios => self.bios_path = path,
            PickTarget::RomFolder => self.add_rom_folder(&path)?,
            // Sem campo de ROM única no editor; fica pronto para a próxima
            // fatia de lançamento por emulador.
            PickTarget::RomFile => return Ok(false),
            PickTarget::GameRom | PickTarget::GameCore | PickTarget::StoreDownloadFolder | PickTarget::SourceCatalog | PickTarget::Artwork => return Ok(false),
            PickTarget::Avatar | PickTarget::Background | PickTarget::Banner => return Ok(false),
        }
        Ok(true)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourcesState {
    pub items: Vec<SourceItem>,
    pub status: LoadStatus,
    pub url_draft: String,
    pub busy: bool,
    pub action_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMode {
    #[default]
    SignIn,
    SignUp,
}

/// Formulário do overlay de login/cadastro.
#[derive(Debug, Clone, Default)]
pub struct AuthState {
    pub open: bool,
    pub required: bool,
    pub mode: AuthMode,
    pub email: String,
    pub username: String,
    pub password: String,
    pub error: Option<String>,
    pub sending: bool,
}

/// Campos de texto do editor de perfil. Banner e fundo entram por upload
/// (`account:setBackground`) e só saem por `ProfileImageCleared`, então não
/// são campos editáveis à mão como no começo do porte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileField {
    DisplayName,
    Summary,
    Country,
    City,
    Showcase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileImageKind {
    Background,
    Banner,
}

impl ProfileImageKind {
    pub fn from_purpose(purpose: &PickPurpose) -> Option<Self> {
        match purpose {
            PickPurpose::Background => Some(Self::Background),
            PickPurpose::Banner => Some(Self::Banner),
            _ => None,
        }
    }

    pub fn ipc_kind(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Banner => "banner",
        }
    }

    /// Nome do campo em `account:updateProfile` (`null` limpa a imagem).
    pub fn ipc_field(self) -> &'static str {
        match self {
            Self::Background => "background_url",
            Self::Banner => "banner_url",
        }
    }

    pub fn success_key(self) -> &'static str {
        match self {
            Self::Background => "native.profile.background_updated",
            Self::Banner => "native.profile.banner_updated",
        }
    }
}

/// Abas do editor de perfil — as quatro do `EditProfile.tsx` da web.
///
/// A ordem é a da barra horizontal (Geral, Avatar, Banner, Fundo) e cada uma
/// desenha um conteúdo próprio; o cabeçalho é comum às quatro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileEditTab {
    #[default]
    General,
    Avatar,
    Banner,
    Background,
}

impl ProfileEditTab {
    pub const ALL: [ProfileEditTab; 4] = [
        Self::General,
        Self::Avatar,
        Self::Banner,
        Self::Background,
    ];

    pub fn label<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            Self::General => "editprofile.nav.geral",
            Self::Avatar => "editprofile.nav.avatar",
            Self::Banner => "editprofile.nav.banner",
            Self::Background => "editprofile.nav.background",
        };
        i18n.t(key)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProfileEditorState {
    pub editing: bool,
    /// Aba visível; sobrevive ao refresh do perfil (só o "editar" reseta).
    pub tab: ProfileEditTab,
    pub display_name: String,
    pub summary: String,
    pub country: String,
    pub city: String,
    pub showcase: String,
    pub background_url: String,
    pub banner_url: String,
    pub avatar_path: String,
    pub saving: bool,
    pub uploading_avatar: bool,
    pub uploading_background: bool,
    pub uploading_banner: bool,
    /// Seletor em voo; só um diálogo por vez.
    pub pick_busy: Option<PickPurpose>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl ProfileEditorState {
    pub fn from_profile(profile: &AccountProfile) -> Self {
        Self {
            editing: false,
            tab: ProfileEditTab::default(),
            display_name: profile.display_name.clone().unwrap_or_default(),
            summary: profile.summary.clone().unwrap_or_default(),
            country: profile.country.clone().unwrap_or_default(),
            city: profile.city.clone().unwrap_or_default(),
            showcase: profile.showcase.join(", "),
            background_url: profile.background_url.clone().unwrap_or_default(),
            banner_url: profile.banner_url.clone().unwrap_or_default(),
            avatar_path: String::new(),
            saving: false,
            uploading_avatar: false,
            uploading_background: false,
            uploading_banner: false,
            pick_busy: None,
            error: None,
            notice: None,
        }
    }

    pub fn set(&mut self, field: ProfileField, value: String) {
        match field {
            ProfileField::DisplayName => self.display_name = value,
            ProfileField::Summary => self.summary = value,
            ProfileField::Country => self.country = value,
            ProfileField::City => self.city = value,
            ProfileField::Showcase => self.showcase = value,
        }
    }

    pub fn image_url_mut(&mut self, kind: ProfileImageKind) -> &mut String {
        match kind {
            ProfileImageKind::Background => &mut self.background_url,
            ProfileImageKind::Banner => &mut self.banner_url,
        }
    }

    pub fn image_upload_busy_mut(&mut self, kind: ProfileImageKind) -> &mut bool {
        match kind {
            ProfileImageKind::Background => &mut self.uploading_background,
            ProfileImageKind::Banner => &mut self.uploading_banner,
        }
    }
}

/// Estado da conta online. `Unknown` é o boot antes de `account:status`;
/// `Loading` cobre login/reload do perfil. `Error` preserva a sessão quando
/// ela existe — falha de perfil não expulsa o usuário.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AccountStatus {
    #[default]
    Unknown,
    SignedOut,
    Loading,
    SignedIn,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct AccountState {
    pub status: AccountStatus,
    pub session: Option<AccountSession>,
    pub profile: Option<AccountProfile>,
    pub avatar: Option<Handle>,
    pub avatar_url: Option<String>,
    /// Avatar local (`profile.avatar` do config) usado quando a conta não
    /// oferece `avatar_url` — o mesmo `||` do `Sidebar.tsx` da web.
    pub fallback_avatar_url: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub auth: AuthState,
    pub editor: ProfileEditorState,
    /// `profile:stats` do card de Estatísticas (`ProfilePage.tsx`).
    pub stats: Option<ProfileStats>,
    /// Primeiro boot sem sessão já abriu o login (não reabrir sozinho).
    pub auto_auth_shown: bool,
}

impl AccountState {
    pub fn is_signed_in(&self) -> bool {
        self.session.is_some()
    }

    /// Nome da sidebar: display_name > username da sessão > email > fallback.
    pub fn display_name<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.profile
            .as_ref()
            .map(AccountProfile::display)
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                self.session
                    .as_ref()
                    .and_then(|session| session.username.as_deref())
            })
            .or_else(|| {
                self.session
                    .as_ref()
                    .and_then(|session| session.email.as_deref())
            })
            .unwrap_or(fallback)
    }

    pub fn initial(&self, fallback: &str) -> String {
        self.display_name(fallback)
            .chars()
            .next()
            .map(|character| character.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string())
    }

    /// Sessão restaurada/renovada: a identidade da sessão pinta na hora e o
    /// perfil completo entra depois.
    pub fn apply_session(&mut self, session: AccountSession) {
        let username = session.username.clone().unwrap_or_default();
        self.session = Some(session);
        self.profile = Some(AccountProfile {
            username,
            ..AccountProfile::default()
        });
        self.avatar = None;
        self.avatar_url = None;
        self.error = None;
        self.notice = None;
        self.status = AccountStatus::Loading;
    }

    pub fn profile_ready(&mut self, profile: AccountProfile) {
        if !self.editor.editing {
            let notice = self.editor.notice.take();
            let error = self.editor.error.take();
            self.editor = ProfileEditorState::from_profile(&profile);
            self.editor.notice = notice;
            self.editor.error = error;
        }
        self.profile = Some(profile);
        self.error = None;
        self.status = AccountStatus::SignedIn;
    }

    pub fn profile_failed(&mut self, error: String) {
        self.error = Some(error);
        self.status = if self.is_signed_in() {
            AccountStatus::Error
        } else {
            AccountStatus::SignedOut
        };
    }

    pub fn signed_out(&mut self) {
        self.status = AccountStatus::SignedOut;
        self.session = None;
        self.profile = None;
        self.avatar = None;
        self.avatar_url = None;
        self.error = None;
        self.notice = None;
        self.editor = ProfileEditorState::default();
    }

    pub fn open_auth(&mut self, required: bool) {
        self.auth.open = true;
        self.auth.required = required;
        self.auth.error = None;
        self.auth.password.clear();
    }

    /// Fecha o overlay se ele não for obrigatório; devolve `true` quando fechou.
    pub fn close_auth(&mut self) -> bool {
        if self.auth.required || self.auth.sending {
            return false;
        }
        self.auth.open = false;
        self.auth.error = None;
        self.auth.password.clear();
        true
    }

    pub fn auth_failed(&mut self, error: String) {
        self.auth.sending = false;
        self.auth.error = Some(error.clone());
        self.error = Some(error);
        if !self.is_signed_in() {
            self.status = AccountStatus::SignedOut;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendAction {
    Send,
    Accept,
    Cancel,
    Remove,
}

impl FriendAction {
    pub fn channel(self) -> &'static str {
        match self {
            FriendAction::Send => "friends:send",
            FriendAction::Accept => "friends:accept",
            FriendAction::Cancel => "friends:cancel",
            FriendAction::Remove => "friends:remove",
        }
    }

    pub fn success_message<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            FriendAction::Send => "amigos.pedido_enviado",
            FriendAction::Accept => "amigos.aceito",
            FriendAction::Cancel => "amigos.recusado",
            FriendAction::Remove => "amigos.removido",
        };
        i18n.t(key)
    }
}

#[derive(Debug, Clone)]
pub struct FriendProfileState {
    pub id: String,
    pub is_friend: bool,
    pub status: LoadStatus,
    pub profile: Option<FriendProfile>,
    pub error: Option<String>,
    /// Preenchido em paralelo quando o alvo é um amigo aceito.
    pub achievements_status: LoadStatus,
    pub achievements: Option<FriendAchievements>,
    pub achievements_error: Option<String>,
    pub confirming_remove: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FriendsState {
    pub data: Option<FriendsLoad>,
    pub status: LoadStatus,
    pub search_query: String,
    pub search_results: Option<Vec<FriendSearchResult>>,
    pub search_status: LoadStatus,
    pub search_requested: Option<String>,
    pub action_busy: Option<String>,
    pub action_error: Option<String>,
    pub notice: Option<String>,
    pub profile: Option<FriendProfileState>,
}

#[derive(Debug, Clone)]
pub struct PluginNotice {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginDetailsPanel {
    pub id: String,
    pub status: LoadStatus,
    pub details: Option<PluginDetails>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ComponentsState {
    pub plugins: Vec<PluginItem>,
    pub status: LoadStatus,
    pub path_draft: String,
    pub registering: bool,
    /// Ação em voo; evita dois IPCs concorrentes no mesmo registro.
    pub busy: Option<String>,
    /// Último resultado por plugin, exibido no card.
    pub notices: HashMap<String, PluginNotice>,
    /// Banner global de registro/erro de ação.
    pub banner: Option<PluginNotice>,
    pub details: Option<PluginDetailsPanel>,
    /// Rascunho da chave Hubcap. O valor salvo chega mascarado; o usuário
    /// cola a chave inteira para substituir.
    pub hubcap_draft: String,
    pub hubcap_saving: bool,
    pub hubcap_validating: bool,
    /// `ok`, `invalida`, `sem_chave` ou `indisponivel`.
    pub hubcap_test: Option<String>,
    pub hubcap_notice: Option<String>,
    /// Uso diário mostrado embaixo do campo, o mesmo de `/api-keys/stats`.
    pub hubcap_stats: Option<HubcapStats>,
    /// Chave em claro desta sessão. O config só devolve a máscara, e a
    /// consulta de uso precisa da chave inteira.
    hubcap_secret: Option<String>,
}

/// Resposta de `GET /api/v1/user/stats`: usuário, uso do dia e limite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubcapStats {
    pub username: Option<String>,
    pub daily_usage: Option<u64>,
    pub daily_limit: Option<u64>,
    pub error: Option<String>,
}

impl HubcapStats {
    pub(crate) fn rejected() -> Self {
        Self {
            username: None,
            daily_usage: None,
            daily_limit: None,
            error: Some("rejected".to_string()),
        }
    }

    pub(crate) fn unreachable() -> Self {
        Self {
            username: None,
            daily_usage: None,
            daily_limit: None,
            error: Some("unreachable".to_string()),
        }
    }

    pub(crate) fn missing() -> Self {
        Self {
            username: None,
            daily_usage: None,
            daily_limit: None,
            error: Some("sem_chave".to_string()),
        }
    }
}

/// Aba interna de Lojas: a mesma alternância `steam | retro` da web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoreTab {
    #[default]
    Steam,
    Retro,
}

impl StoreTab {
    pub fn label<'a>(self, i18n: &'a I18n) -> &'a str {
        let key = match self {
            StoreTab::Steam => "store.titulo",
            StoreTab::Retro => "store.retro_tab",
        };
        i18n.t(key)
    }
}

#[derive(Debug, Clone, Default)]
pub struct StoreSearchState {
    /// Termo efetivamente enviado ao dispatcher (pode diferir do rascunho).
    pub query: String,
    pub status: LoadStatus,
    pub result: Option<StoreSearch>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreBrowseState {
    pub games: Vec<StoreGame>,
    pub status: LoadStatus,
    pub total: Option<u64>,
    pub has_more: bool,
    pub next_offset: Option<u64>,
    pub loading_more: bool,
    generation: u64,
    next_page: Option<NextStorePage>,
    /// Falha de "carregar mais" sem apagar a grade já visível.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
enum NextStorePage {
    Loading,
    Ready(StorePage),
}

#[derive(Debug, Clone)]
pub struct StoreDetailState {
    pub game: StoreGame,
    pub info: Option<StoreInfo>,
    pub status: LoadStatus,
    pub error: Option<String>,
    /// Manifesto de `store:installInfo` (depots, token, tamanho, installdir).
    pub install: Option<StoreInstallInfo>,
    pub install_status: LoadStatus,
    pub install_error: Option<String>,
    /// Bibliotecas de `store:libraries`, usadas para escolher o `steamDir`.
    pub libraries: Vec<StoreLibrary>,
    pub libraries_status: LoadStatus,
    pub libraries_error: Option<String>,
    pub selected_library: usize,
    pub selected_depots: BTreeSet<String>,
    pub install_busy: bool,
    /// Confirmação exibida depois que `store:install` aceita o pedido.
    pub install_message: Option<String>,
    /// Captura selecionada na galeria da ficha da loja.
    pub selected_screenshot: usize,
    pub downloads: StoreDownloadsState,
}

#[derive(Debug, Clone, Default)]
pub struct StoreDownloadsState {
    pub generation: u64,
    pub status: LoadStatus,
    pub data: SourceOffers,
    pub error: Option<String>,
    pub open: bool,
    pub selected_offer: usize,
    pub selected_uri: usize,
    pub path: String,
    pub pick_busy: bool,
}

#[derive(Debug, Clone)]
pub struct RetroDetailState {
    /// Jogo semeado pelo card; a ficha completa substitui/completa os campos.
    pub game: RetroGame,
    pub detail: Option<RetroGameDetail>,
    pub status: LoadStatus,
    pub error: Option<String>,
    pub selected_offer: Option<String>,
    pub offer_detail: Option<RetroOfferDetail>,
    pub offer_status: LoadStatus,
    pub offer_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreState {
    pub tab: StoreTab,
    pub query: String,
    pub suggestions: Vec<StoreGame>,
    /// Guarda o termo da última consulta de sugestão para não repetir IPC.
    pub suggested_query: String,
    pub search: Option<StoreSearchState>,
    pub browse: StoreBrowseState,
    pub detail: Option<StoreDetailState>,
    pub detail_generation: u64,
}

/// Vitrine da Home (`store:recent` de `top100in2weeks`), separada da Loja
/// para que navegar entre as duas abas não descarte a lista uma da outra.
#[derive(Debug, Clone, Default)]
pub struct HomeState {
    pub games: Vec<StoreGame>,
    pub status: LoadStatus,
    pub error: Option<String>,
    /// `game:sysinfo` do herói em exibição (descrição/backdrop da loja).
    pub hero_appid: Option<String>,
    pub hero_info: Option<StoreInfo>,
    pub hero_status: LoadStatus,
}

#[derive(Debug, Clone)]
pub struct RetroState {
    pub query: String,
    /// Termo efetivamente aplicado à grade; paginar não usa o rascunho.
    pub applied_query: String,
    pub system: RetroSystemInfo,
    pub games: Vec<RetroGame>,
    pub sources: Vec<RetroSource>,
    pub status: LoadStatus,
    pub total: Option<u64>,
    pub total_offers: Option<u64>,
    pub offset: u64,
    pub has_more: bool,
    pub error: Option<String>,
    /// Ficha aberta por clique num card; `None` na grade.
    pub detail: Option<RetroDetailState>,
    /// Descarta respostas de uma página antiga quando o usuário pagina rápido.
    pub generation: u64,
    showing_cached_page: bool,
}

impl Default for RetroState {
    fn default() -> Self {
        Self {
            query: String::new(),
            applied_query: String::new(),
            system: crate::backend::RETRO_ALL_SYSTEMS,
            games: Vec::new(),
            sources: Vec::new(),
            status: LoadStatus::Idle,
            total: None,
            total_offers: None,
            offset: 0,
            has_more: false,
            error: None,
            detail: None,
            generation: 0,
            showing_cached_page: false,
        }
    }
}

/// Tipo de update que o diálogo está mostrando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateDialogKind {
    Packaged,
    Git,
}

/// Estágio em que uma ação do updater está (ações ficam desabilitadas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    Downloading,
    Applying,
    Installing,
    Restarting,
}

/// Conteúdo congelado do diálogo; o snapshot vivo só substitui `packaged`.
#[derive(Debug, Clone)]
pub struct UpdateDialogState {
    pub kind: UpdateDialogKind,
    pub channel: String,
    pub git: Option<UpdateCheck>,
    pub packaged: Option<PackagedState>,
    pub stage: Option<UpdateStage>,
    pub error: Option<String>,
}

impl UpdateDialogState {
    pub fn is_busy(&self) -> bool {
        self.stage.is_some()
    }
}

/// Estado de atualização compartilhado pelo diálogo e pela aba Sobre.
#[derive(Debug, Default)]
pub struct UpdateUiState {
    pub snapshot: Option<UpdateSnapshot>,
    pub checking: bool,
    pub last_check: Option<String>,
    pub last_error: Option<String>,
    pub available_version: Option<String>,
    pub last_motivo: Option<String>,
    /// Versão dispensada nesta sessão; não reabre sozinha.
    pub dismissed_version: Option<String>,
    pub dialog: Option<UpdateDialogState>,
    pub backdrop: Option<Handle>,
    pub backdrop_waiting: bool,
    backdrop_generation: u64,
    backdrop_capture_requested: Option<u64>,
    /// Evita empilhar polls de snapshot enquanto o updater está ocupado.
    pub poll_in_flight: bool,
}

/// Overlay de boot; o player é separado do trailer para não roubá-lo.
pub struct BootSplash {
    pub active: bool,
    pub media: MediaPlayer,
    pub started: Instant,
    pub path: Option<String>,
}

impl Default for BootSplash {
    fn default() -> Self {
        Self {
            active: false,
            media: MediaPlayer::new(),
            started: Instant::now(),
            path: None,
        }
    }
}

pub const STORE_PAGE_SIZE: u64 = 24;
const STORE_THUMBNAIL_AHEAD: usize = 10;
const STORE_THUMBNAIL_JOBS: usize = 3;
pub const RETRO_PAGE_SIZE: u64 = 24;

pub struct App {
    pub backend: Backend,
    pub screen: Screen,
    pub library: Library,
    pub covers: CoverCache,
    retro_library_art: HashMap<String, Option<(String, Option<String>)>>,
    pub store_thumbnails: CoverCache,
    store_thumbnail_queue: VecDeque<String>,
    store_thumbnail_jobs: usize,
    /// Poster → mídia de vídeo do mesmo card de seção.
    ///
    /// A Steam manda um `poster.avif` que às vezes é um placeholder preto
    /// (Black Myth: Wukong: 336 bytes, luminância 0). O vídeo tem a arte real,
    /// e o app extrai o primeiro frame visível para substituir o poster.
    pub card_videos: std::collections::HashMap<String, String>,
    /// URLs de poster que decodificaram como imagem preta — placeholder da
    /// Steam. Só esses vale a pena trocar pelo frame do vídeo: baixar e
    /// decodificar 4 MB de vídeo para um poster legítimo seria desperdício.
    pub poster_is_black: std::collections::HashSet<String>,
    /// Cards de seção animados: os `<video>` do `about` tocando em loop, como
    /// na página original da Steam. Limitado a poucos players simultâneos.
    pub animated_cards: crate::animated_cards::AnimatedCards,
    pub friend_media: crate::animated_cards::AnimatedCards,
    pub search: String,
    /// Idioma ativo do shell; carregado do `config:get` no boot.
    pub i18n: I18n,
    pub media: MediaPlayer,
    /// `None` enquanto o `game:sysinfo` está em voo.
    pub game_media: Option<GameMedia>,
    game_media_cache: HashMap<String, (Instant, GameMedia)>,
    store_info_cache: HashMap<String, (Instant, StoreInfo)>,
    detail_prefetching: BTreeSet<String>,
    /// Índice da screenshot selecionada na galeria da ficha.
    pub selected_screenshot: usize,
    /// `true` quando a galeria mostra o trailer; `false` mostra a screenshot.
    pub gallery_trailer: bool,
    /// Aba Mínimos/Recomendados compartilhada pelas fichas.
    pub requirements_tab: RequirementsTab,
    /// Painéis da ficha por jogo (conquistas, ProtonDB, stats/reviews).
    /// O mapa é o cache da sessão: reabrir um jogo não refaz o IPC.
    pub game_panels: HashMap<String, GamePanels>,
    /// Seletor de trailers do YouTube sobre a ficha.
    pub trailer_picker: TrailerPickerState,
    /// O usuário abriu um trailer local; o `game:sysinfo` em voo não pode
    /// trocar o pipeline pelo HLS da Steam.
    pub trailer_override: bool,
    pub downloads: DownloadsState,
    pub settings: SettingsState,
    pub emulation: EmulationState,
    pub sources: SourcesState,
    pub account: AccountState,
    pub friends: FriendsState,
    pub components: ComponentsState,
    pub community: CommunityCache,
    pub store: StoreState,
    pub console_storefront: views::big_picture_store::StorefrontState,
    pub home: HomeState,
    pub page_entered: Instant,
    pub page_direction: f32,
    pub home_entered: Instant,
    pub home_hero_started: Instant,
    pub home_hero_index: usize,

    pub launch_reduce_motion: bool,
    pub retro: RetroState,
    pub sidebar_search: String,
    pub sidebar_games_open: bool,
    /// Menu "meu perfil" expandido na sidebar (bloco `profileMenu` do web).
    pub profile_menu_open: bool,
    pub window: Option<window::Id>,
    /// Tamanho atual da janela; alimenta o scroll centralizado do trilho.
    pub window_size: Size,
    /// Estado do modo console (gamepad, foco, aba, painel).
    pub big_picture: BigPicture,
    /// Feed da aba Notícias do console.
    pub console_news: NewsState,
    /// Jogo cujo `game:sysinfo` alimenta o trailer do painel do console.
    pub console_media_id: Option<String>,
    /// Escrita de favorito em voo; evita dois `overrides:set` concorrentes.
    pub favorite_busy: Option<String>,
    /// Refresh leve da biblioteca em voo; evita polls sobrepostos.
    library_poll_in_flight: bool,
    library_dirty: bool,
    section_loaded_at: HashMap<(Section, bool), Instant>,
    news_loaded_at: Option<Instant>,
    /// Geração do último refresh/carga aceito. Resposta com geração antiga é
    /// descartada e não mexe no flag de uma requisição mais nova.
    library_epoch: u64,
    /// Sessão de jogo: abrindo, rodando, parando e última falha.
    pub launch: GameLaunchState,
    /// Diálogo de opções de inicialização (Wine/Proton, env, args).
    pub launch_options: LaunchOptionsState,
    /// Menu de contexto da biblioteca (um por vez).
    pub game_menu: Option<GameMenuState>,
    /// Geração do menu; resultados assíncronos só fecham a instância que os
    /// disparou.
    pub game_menu_serial: u64,
    /// Editor de App/Jogo aberto pelo menu.
    pub game_edit: Option<GameEditState>,
    /// Modal de metadados (título/descrição/três artes).
    pub game_metadata: Option<MetadataState>,
    /// Escrita de overrides em voo e geração para descartar respostas antigas.
    pub library_overrides: LibraryOverrideState,
    /// Tela desktop a restaurar quando o console for fechado.
    pub previous_screen: Option<Screen>,
    /// Splash de boot (vídeo opcional, no máximo uma vez por processo).
    pub boot: BootSplash,
    /// Estado de atualização compartilhado pelo diálogo e pela aba Sobre.
    pub update: UpdateUiState,
    /// Toasts de conquista; vazio quando `notifications_enabled=false`.
    pub toasts: ToastQueue,
    pub notifications_enabled: bool,
    pub hide_changelog_on_start: bool,
}

impl App {
    pub fn boot(backend: Backend) -> (Self, Task<Message>) {
        let mut app = Self {
            backend,
            screen: Screen::Section(Section::Home),
            library: Library::loading(),
            covers: CoverCache::default(),
            retro_library_art: HashMap::new(),
            store_thumbnails: CoverCache::with_budget(16 * 1024 * 1024),
            store_thumbnail_queue: VecDeque::new(),
            store_thumbnail_jobs: 0,
            card_videos: std::collections::HashMap::new(),
            poster_is_black: std::collections::HashSet::new(),
            animated_cards: crate::animated_cards::AnimatedCards::default(),
            friend_media: crate::animated_cards::AnimatedCards::default(),
            search: String::new(),
            i18n: I18n::default(),
            media: MediaPlayer::new(),
            game_media: None,
            game_media_cache: HashMap::new(),
            store_info_cache: HashMap::new(),
            detail_prefetching: BTreeSet::new(),
            selected_screenshot: 0,
            gallery_trailer: false,
            requirements_tab: RequirementsTab::default(),
            game_panels: HashMap::new(),
            trailer_picker: TrailerPickerState::default(),
            trailer_override: false,
            downloads: DownloadsState::default(),
            settings: SettingsState::default(),
            emulation: EmulationState::default(),
            sources: SourcesState::default(),
            account: AccountState::default(),
            friends: FriendsState::default(),
            components: ComponentsState::default(),
            community: CommunityCache::default(),
            store: StoreState::default(),
            console_storefront: views::big_picture_store::StorefrontState::default(),
            home: HomeState::default(),
            page_entered: Instant::now(),
            page_direction: 1.0,
            home_entered: Instant::now(),
            home_hero_started: Instant::now(),
            home_hero_index: 0,

            launch_reduce_motion: false,
            retro: RetroState::default(),
            sidebar_search: String::new(),
            sidebar_games_open: true,
            profile_menu_open: false,
            window: None,
            window_size: Size::new(1280.0, 800.0),
            big_picture: BigPicture::new(),
            console_news: NewsState::default(),
            console_media_id: None,
            favorite_busy: None,
            library_poll_in_flight: false,
            library_dirty: false,
            section_loaded_at: HashMap::new(),
            news_loaded_at: None,
            library_epoch: 0,
            launch: GameLaunchState::default(),
            launch_options: LaunchOptionsState::default(),
            game_menu: None,
            game_menu_serial: 0,
            game_edit: None,
            game_metadata: None,
            library_overrides: LibraryOverrideState::default(),
            previous_screen: None,
            boot: BootSplash::default(),
            update: UpdateUiState::default(),
            toasts: ToastQueue::new(),
            notifications_enabled: true,
            hide_changelog_on_start: false,
        };
        let launch_config = {
            let backend = app.backend.clone();
            Task::perform(
                async move { backend.load_settings().await },
                Message::LaunchConfigLoaded,
            )
        };
        let update_snapshot = {
            let backend = app.backend.clone();
            Task::perform(
                async move { backend.load_update_snapshot().await },
                Message::UpdateSnapshotLoaded,
            )
        };
        let account_status = {
            let backend = app.backend.clone();
            Task::perform(
                async move { backend.load_account_status().await },
                Message::AccountStatusLoaded,
            )
        };
        let home_task = app.load_section(Section::Home, false);
        // A própria carga já deixa a página e as primeiras miniaturas prontas.
        let store_task = app.load_store_browse(false);
        let retro_task = app.load_retro_page(0);

        let task = Task::batch([
            app.reload_task(),
            home_task,
            window::latest().map(Message::WindowReady),
            launch_config,
            update_snapshot,
            account_status,
            store_task,
            retro_task,
        ]);
        (app, task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LibraryLoaded(Ok(games)) => {
                eprintln!("[arcadia-native] biblioteca: {} jogo(s)", games.len());
                self.library_epoch = self.library_epoch.wrapping_add(1);
                self.library_poll_in_flight = false;
                self.apply_library_snapshot(games)
            }
            Message::LibraryLoaded(Err(error)) => {
                eprintln!("[arcadia-native] falha ao carregar biblioteca: {error}");
                self.library_epoch = self.library_epoch.wrapping_add(1);
                self.library_poll_in_flight = false;
                self.library.error(error);
                Task::none()
            }
            Message::LibraryRefreshed { epoch, result } => {
                // Resposta de uma leitura já superada não pode ressuscitar a
                // lista antiga nem limpar o flag do refresh mais novo.
                if epoch != self.library_epoch {
                    return Task::none();
                }
                self.library_poll_in_flight = false;
                match result {
                    Ok(games) => self.apply_library_snapshot(games),
                    Err(error) => {
                        // Falha de um refresh em segundo plano não troca a
                        // grade por uma tela de erro; o próximo foco/poll
                        // tenta de novo.
                        eprintln!("[arcadia-native] refresh leve da biblioteca falhou: {error}");
                        Task::none()
                    }
                }
            }
            Message::SteamLibraryPoll => self.light_library_refresh(),
            Message::RefreshVisibleSection => self.refresh_visible_section(),
            Message::OpenGameContextMenu { game_id, anchor } => {
                if self.library.game(&game_id).is_none()
                    || self.game_edit.is_some()
                    || self.game_metadata.is_some()
                    || self.launch_options.open
                    || self.account.auth.open
                    || self.update.dialog.is_some()
                    || self.boot.active
                {
                    return Task::none();
                }
                self.game_menu_serial = self.game_menu_serial.wrapping_add(1);
                self.game_menu = Some(GameMenuState::with_instance(
                    game_id,
                    anchor,
                    self.game_menu_serial,
                ));
                Task::none()
            }
            Message::CloseGameContextMenu => {
                self.game_menu = None;
                Task::none()
            }
            Message::GameMenuAction(action) => self.game_menu_action(action),
            Message::GameMenuCategoryDraftChanged(value) => {
                if let Some(menu) = self.game_menu.as_mut() {
                    menu.category_draft = value;
                }
                Task::none()
            }
            Message::GameMenuCategoryToggled(category) => self.menu_toggle_category(category),
            Message::GameMenuCategoryAdded => self.menu_add_category(),
            Message::GameMenuCategoryBack => {
                if let Some(menu) = self.game_menu.as_mut() {
                    menu.categories_open = false;
                }
                Task::none()
            }
            Message::GameMenuLogsDone {
                game_id,
                instance,
                result,
            } => {
                if !self.menu_instance_matches(&game_id, instance) {
                    return Task::none();
                }
                match result {
                    Ok(GameLogOutcome::Opened) => self.game_menu = None,
                    Ok(outcome) => {
                        let notice = match outcome {
                            GameLogOutcome::NoLogs => MenuNotice::NoLogs,
                            GameLogOutcome::InvalidId | GameLogOutcome::Error(_) => {
                                MenuNotice::LogsFailed
                            }
                            GameLogOutcome::Opened => return Task::none(),
                        };
                        if let Some(menu) = self
                            .game_menu
                            .as_mut()
                            .filter(|menu| menu.game_id == game_id && menu.instance == instance)
                        {
                            menu.notice = Some(notice);
                        }
                    }
                    Err(_) => {
                        if let Some(menu) = self
                            .game_menu
                            .as_mut()
                            .filter(|menu| menu.game_id == game_id && menu.instance == instance)
                        {
                            menu.notice = Some(MenuNotice::LogsFailed);
                        }
                    }
                }
                Task::none()
            }
            Message::GameMenuInstallDone {
                game_id,
                instance,
                result,
            } => {
                if !self.menu_instance_matches(&game_id, instance) {
                    return Task::none();
                }
                match result {
                    Ok(()) => self.game_menu = None,
                    Err(_) => {
                        if let Some(menu) = self
                            .game_menu
                            .as_mut()
                            .filter(|menu| menu.game_id == game_id && menu.instance == instance)
                        {
                            menu.notice = Some(MenuNotice::SteamOpenFailed);
                        }
                    }
                }
                Task::none()
            }
            Message::GameMenuKey(event) => {
                let escape = matches!(
                    event,
                    keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }
                );
                if !escape {
                    return Task::none();
                }
                if self.game_metadata.is_some() {
                    self.game_metadata = None;
                } else if self.game_edit.is_some() {
                    self.game_edit = None;
                } else {
                    self.game_menu = None;
                }
                Task::none()
            }
            Message::GameOverridesSaved {
                epoch,
                account,
                game_id,
                result,
            } => self.finish_library_override(epoch, account, game_id, result),
            Message::LibraryShowHiddenToggled => {
                self.library.show_hidden = !self.library.show_hidden;
                self.library.notice = None;
                Task::none()
            }
            Message::DismissLibraryNotice => {
                self.library.notice = None;
                Task::none()
            }
            Message::GameEditTitleChanged(value) => {
                if let Some(edit) = self.game_edit.as_mut() {
                    edit.title = value;
                    edit.error = None;
                }
                Task::none()
            }
            Message::GameEditDescriptionChanged(value) => {
                if let Some(edit) = self.game_edit.as_mut() {
                    edit.description = value;
                    edit.description_reset = false;
                    edit.error = None;
                }
                Task::none()
            }
            Message::GameEditDescriptionReset => {
                if let Some(edit) = self.game_edit.as_mut() {
                    edit.description_reset = true;
                    edit.error = None;
                }
                Task::none()
            }
            Message::GameEditPlatformChanged(platform) => {
                if let Some(edit) = self.game_edit.as_mut() {
                    edit.platform = Some(platform);
                    edit.error = None;
                }
                Task::none()
            }
            Message::GameEditExeChanged(value) => {
                if let Some(edit) = self.game_edit.as_mut() {
                    edit.exe = value;
                    edit.error = None;
                }
                Task::none()
            }
            Message::GameEditSave => self.save_game_edit(),
            Message::GameEditCancel => {
                self.game_edit = None;
                Task::none()
            }
            Message::GameEditSaved {
                game_id,
                request,
                account,
                result,
            } => self.finish_game_edit(game_id, request, account, result),
            Message::GameMetadataTitleChanged(value) => {
                if let Some(metadata) = self.game_metadata.as_mut() {
                    metadata.title = value;
                    metadata.error = None;
                }
                Task::none()
            }
            Message::GameMetadataDescriptionChanged(value) => {
                if let Some(metadata) = self.game_metadata.as_mut() {
                    metadata.description = value;
                    metadata.description_reset = false;
                    metadata.error = None;
                }
                Task::none()
            }
            Message::GameMetadataDescriptionReset => {
                if let Some(metadata) = self.game_metadata.as_mut() {
                    metadata.description_reset = true;
                    metadata.error = None;
                }
                Task::none()
            }
            Message::GameMetadataArtReset(kind) => {
                if let Some(metadata) = self.game_metadata.as_mut() {
                    metadata.reset_art(kind);
                    metadata.error = None;
                }
                Task::none()
            }
            Message::GameMetadataSave => self.save_game_metadata(),
            Message::GameMetadataCancel => {
                self.game_metadata = None;
                Task::none()
            }
            Message::GameMetadataSaved {
                game_id,
                request,
                account,
                result,
            } => self.finish_game_metadata(game_id, request, account, result),
            Message::CoverLoaded { url, loaded } => {
                // Poster decodificado: se for preto puro, a Steam mandou um
                // placeholder. O vídeo do mesmo card tem a arte, então o frame
                // dele substitui o poster.
                if let Some((handle, (width, height), black)) = loaded {
                    self.covers.insert(url.clone(), handle, (width, height));

                    if black {
                        self.poster_is_black.insert(url.clone());
                        if let Some(video) = self.card_videos.get(&url).cloned() {
                            return Task::perform(
                                async move { extract_first_visible_frame(&video) },
                                move |frame| Message::PosterFrameLoaded { poster: url, frame },
                            );
                        }
                    }
                } else {
                    // Sem log, uma capa que nunca carrega é invisível: a UI
                    // mostra o placeholder e ninguém sabe se foi rede, 404 ou
                    // formato que o crate `image` não abre. A URL vai na
                    // mensagem para dar para testar com `curl`.
                    eprintln!("[arcadia-native] capa falhou: {url}");
                    self.covers.fail(&url);
                }
                Task::none()
            }
            Message::StoreThumbnailVisible(url) => {
                if self.desktop_store_visible() {
                    self.store_thumbnail_queue.retain(|queued| queued != &url);
                    self.store_thumbnail_queue.push_front(url);
                    self.run_store_thumbnail_queue()
                } else {
                    Task::none()
                }
            }
            Message::StoreThumbnailHidden(url) => {
                self.store_thumbnail_queue.retain(|queued| queued != &url);
                Task::none()
            }
            Message::StoreThumbnailLoaded { url, image } => {
                if let Some((handle, size)) = image {
                    self.store_thumbnails.insert(url, handle, size);
                } else {
                    self.store_thumbnails.fail(&url);
                }
                self.store_thumbnail_jobs -= 1;
                self.run_store_thumbnail_queue()
            }
            Message::HomeArtTick => {
                if matches!(self.screen, Screen::Section(Section::Home)) && self.store.detail.is_none()
                    && self.home_hero_started.elapsed() >= Duration::from_secs(6)
                    && self.home.games.len() > 1
                {
                    self.home_hero_index = (self.home_hero_index + 1) % self.home.games.len().min(5);
                    self.home_hero_started = Instant::now();
                    if let Some(game) = self.home.games.get(self.home_hero_index).cloned() {
                        return self.home_hero_info_task(&game);
                    }
                }
                Task::none()
            },
            Message::CardsTick => {
                self.animated_cards.tick();
                if matches!(self.screen,Screen::Section(Section::Friends)) {self.friend_media.tick();}
                Task::none()
            }
            Message::PosterFrameLoaded { poster, frame } => {
                if let Some((width, height, pixels)) = frame {
                    self.covers.insert(
                        poster,
                        Handle::from_rgba(width, height, pixels),
                        (width, height),
                    );
                }
                Task::none()
            }
            Message::Reload => {
                self.profile_menu_open = false;
                self.library.set_loading();
                self.reload_task()
            }
            Message::SearchChanged(search) => {
                self.search = search;
                Task::none()
            }
            Message::Navigate(section) => {
                self.game_menu = None;
                if section != Section::Stores {
                    self.store_thumbnail_queue.clear();
                }
                if section == Section::BigPicture {
                    return self.enter_big_picture();
                }
                self.media.clear();
                self.game_media = None;
                self.reset_trailer_state();
                // Só os vídeos da ficha pertencem à aba. Avatar da sidebar
                // e quadros do perfil permanecem prontos, sem novo download
                // ou decode ao voltar para My profile.
                self.animated_cards.clear_cards();
                if section != Section::Stores {
                    self.store.detail = None;
                    self.retro.detail = None;
                }
                let next = Screen::for_section(section);
                if self.screen != next {
                    let old_index = match self.screen {
                        Screen::Section(old) => Section::ALL.iter().position(|item| *item == old),
                        Screen::Library => Some(1),
                        _ => None,
                    };
                    let new_index = Section::ALL.iter().position(|item| *item == section);
                    self.page_direction = if new_index < old_index { -1.0 } else { 1.0 };
                    self.page_entered = Instant::now();
                    if section == Section::Home {
                        self.home_entered = Instant::now();
                        self.home_hero_started = Instant::now();
                    }
                }
                self.screen = next;
                match section {
                    Section::Library => self.light_library_refresh(),
                    Section::Downloads | Section::Settings | Section::Friends => self.load_section(section, false),
                    _ => self.refresh_visible_section(),
                }
            }
            Message::OpenGame(id) => {
                self.game_menu = None;
                self.open_game(id)
            }
            Message::BackToLibrary => {
                self.game_menu = None;
                self.media.clear();
                self.game_media = None;
                self.animated_cards.clear_cards();
                self.card_videos.clear();
                self.reset_trailer_state();
                self.screen.back_to_library();
                self.page_entered = Instant::now();
                self.page_direction = -1.0;
                Task::none()
            }
            Message::Refresh(section) => self.load_section(section, true),
            Message::DownloadsLoaded { revisions, result } => {
                match result {
                    Ok(data) => self.downloads.apply_snapshot(revisions, data),
                    Err(_) if revisions != self.downloads.revisions => {
                        // Eventos mais novos já atualizaram a lista.
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] downloads: {error}");
                        if self.downloads.status == LoadStatus::Ready {
                            self.downloads.data.warnings = vec![error];
                        } else {
                            self.downloads.status = LoadStatus::Error(error);
                        }
                    }
                }
                Task::none()
            }
            Message::DownloadActionRequested { item, action } => {
                if self.downloads.busy_id.is_some() {
                    return Task::none();
                }
                self.downloads.busy_id = Some(item.id.clone());
                self.downloads.action_error = None;
                let backend = self.backend.clone();
                let id = item.id;
                let is_torrent = item.is_torrent;
                Task::perform(
                    async move { backend.run_download_action(id, is_torrent, action).await },
                    Message::DownloadActionDone,
                )
            }
            Message::DownloadActionDone(result) => {
                self.downloads.busy_id = None;
                match result {
                    Ok(()) => self.downloads_task(),
                    Err(error) => {
                        self.downloads.action_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::SettingsLoaded(result) => {
                match result {
                    Ok(config) => {
                        self.i18n
                            .set_lang(Lang::from_code(&config.language).unwrap_or_default());
                        self.settings.accent_draft = config.accent.clone();
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.notifications_enabled = config.notifications_enabled;
                        self.hide_changelog_on_start = config.hide_changelog_on_start;
                        if !config.notifications_enabled {
                            self.toasts.clear();
                        }
                        // Tokens de debrid: o editor começa com o que está
                        // salvo (já mascarado pelo `config:get`).
                        self.settings.debrid_drafts.sync_from(&config);
                        // A chave do SteamGridDB e os cookies também vivem na
                        // config; o campo começa com o valor salvo.
                        self.settings.steamgriddb_draft = config.steamgriddb_api_key.clone();
                        self.settings.youtube_cookies_draft = config.youtube_cookies.clone();
                        self.settings.config = Some(config);
                        self.settings.status = LoadStatus::Ready;
                        // O avatar local (`profile.avatar`) entra junto com o
                        // config; sem sessão ele é quem pinta a sidebar.
                        self.sync_fallback_avatar()
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] configurações: {error}");
                        self.settings.status = LoadStatus::Error(error);
                        Task::none()
                    }
                }
            }
            Message::SettingsSaved { key, result } => {
                self.settings.saving = None;
                match result {
                    Ok(config) => {
                        self.i18n
                            .set_lang(Lang::from_code(&config.language).unwrap_or_default());
                        self.settings.accent_draft = config.accent.clone();
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.notifications_enabled = config.notifications_enabled;
                        self.hide_changelog_on_start = config.hide_changelog_on_start;
                        if !config.notifications_enabled {
                            self.toasts.clear();
                        }
                        // Tokens de debrid: o editor começa com o que está
                        // salvo (já mascarado pelo `config:get`).
                        self.settings.debrid_drafts.sync_from(&config);
                        // A chave do SteamGridDB e os cookies também vivem na
                        // config; o campo começa com o valor salvo.
                        self.settings.steamgriddb_draft = config.steamgriddb_api_key.clone();
                        self.settings.youtube_cookies_draft = config.youtube_cookies.clone();
                        self.settings.config = Some(config);
                        self.settings.status = LoadStatus::Ready;
                        self.settings.save_error = None;
                    }
                    Err(error) => {
                        // O seletor trocou o idioma antes do round-trip; se o
                        // config:set falhou, volta para o valor persistido.
                        if key == "language" {
                            if let Some(config) = &self.settings.config {
                                self.i18n.set_lang(
                                    Lang::from_code(&config.language).unwrap_or_default(),
                                );
                            }
                        }
                        self.settings.save_error = Some(self.i18n.t_args(
                            "native.settings.save_error",
                            &[("key", &key), ("error", &error)],
                        ));
                    }
                }
                Task::none()
            }
            Message::SettingsTabChanged(tab) => {
                self.settings.tab = tab;
                if tab == SettingsTab::Emulation
                    && matches!(self.emulation.status, LoadStatus::Idle)
                {
                    self.load_emulation(false)
                } else {
                    Task::none()
                }
            }
            Message::AccentChanged(accent) => {
                self.settings.accent_draft = accent;
                Task::none()
            }
            Message::AccentSave => {
                let accent = self.settings.accent_draft.trim().to_string();
                self.save_setting("accent", Value::String(accent))
            }
            Message::LanguageChanged(language) => {
                if let Some(lang) = Lang::from_code(&language) {
                    self.i18n.set_lang(lang);
                }
                self.save_setting("language", Value::String(language))
            }
            Message::SettingToggled { key, value } => self.save_setting(key, Value::Bool(value)),
            Message::LibraryTabSelected(tab) => {
                self.library.tab = tab;
                Task::none()
            }
            Message::DebridTokenChanged { provider, value } => {
                self.settings.debrid_drafts.set(provider, value);
                // Qualquer edição invalida o "Salvo!" anterior: senão o botão
                // continuaria dizendo "Salvo!" com texto novo por salvar.
                self.settings.debrid_saved = None;
                Task::none()
            }
            Message::DebridToggled(provider) => {
                if self.settings.debrid_open.contains(&provider) {
                    self.settings.debrid_open.remove(&provider);
                } else {
                    self.settings.debrid_open.insert(provider);
                }
                Task::none()
            }
            Message::DebridTokenSave(provider) => {
                let token = self.settings.debrid_drafts.get(provider).trim().to_string();
                let key = provider.config_key();
                self.settings.debrid_saved = Some(provider);
                // Token vazio vira `null` no `prepare_patch` do IPC, que é o
                // contrato de "desconectar" da web.
                let value = if token.is_empty() {
                    Value::Null
                } else {
                    Value::String(token)
                };
                self.save_dynamic_setting(key, value)
            }
            Message::SteamGridDbKeyChanged(value) => {
                self.settings.steamgriddb_draft = value;
                self.settings.steamgriddb_saved = false;
                Task::none()
            }
            Message::SteamGridDbKeySave => {
                let key = self.settings.steamgriddb_draft.trim().to_string();
                self.settings.steamgriddb_saved = true;
                // Chave vazia vira `null` no `prepare_patch`: remove a chave.
                let value = if key.is_empty() {
                    Value::Null
                } else {
                    Value::String(key)
                };
                self.save_dynamic_setting("steamgriddb_api_key", value)
            }
            Message::YoutubeCookiesChanged(value) => {
                self.settings.youtube_cookies_draft = value;
                self.settings.youtube_cookies_saved = false;
                Task::none()
            }
            Message::YoutubeCookiesSave => {
                let cookies = self.settings.youtube_cookies_draft.trim().to_string();
                self.settings.youtube_cookies_saved = true;
                let value = if cookies.is_empty() {
                    Value::Null
                } else {
                    Value::String(cookies)
                };
                self.save_dynamic_setting("youtube_cookies", value)
            }
            Message::TrailerDownloadAll => {
                let backend = self.backend.clone();
                self.settings.trailer_download_all = Some(
                    self.i18n.t("settings.trailers.iniciando").to_string(),
                );
                Task::perform(
                    async move { backend.trailer_download_all().await },
                    Message::TrailerDownloadAllDone,
                )
            }
            Message::TrailerDownloadAllDone(result) => {
                match result {
                    Ok(summary) => self.settings.trailer_download_all = Some(summary),
                    Err(error) => {
                        eprintln!("[arcadia-native] trailer:downloadAll falhou: {error}");
                        self.settings.trailer_download_all = Some(error);
                    }
                }
                Task::none()
            }
            Message::CpuCoresChanged(delta) => {
                let current = self
                    .settings
                    .config
                    .as_ref()
                    .map(|config| config.download_cpu_cores)
                    .unwrap_or(0) as i64;
                let next = (current + delta).clamp(0, 64);
                if next == current {
                    return Task::none();
                }
                self.save_setting("download_cpu_cores", json!(next))
            }
            Message::EmulationLoaded(result) => {
                match result {
                    Ok(load) => {
                        self.emulation.emulators = load.emulators;
                        self.emulation.statuses = load.statuses;
                        self.emulation.rom_index = load.rom_index;
                        self.emulation.warnings = load.warnings;
                        self.emulation.status = LoadStatus::Ready;
                        self.refresh_emulation_editor();
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] emuladores: {error}");
                        self.emulation.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::EmulationDetect => self.load_emulation(true),
            Message::EmulationOpenEditor(id) => {
                self.open_emulation_editor(&id);
                Task::none()
            }
            Message::EmulationCloseEditor => {
                self.emulation.editor = None;
                Task::none()
            }
            Message::EmulationExecutableChanged(value) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.executable = value;
                }
                Task::none()
            }
            Message::EmulationCorePathChanged(value) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.core_path = value;
                }
                Task::none()
            }
            Message::EmulationBiosPathChanged(value) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.bios_path = value;
                }
                Task::none()
            }
            Message::EmulationArgsChanged(value) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.args_text = value;
                }
                Task::none()
            }
            Message::EmulationFolderDraftChanged(value) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.folder_draft = value;
                }
                Task::none()
            }
            Message::EmulationFolderAdd => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    let draft = editor.folder_draft.clone();
                    match editor.add_rom_folder(&draft) {
                        Ok(()) => {
                            editor.folder_draft.clear();
                            editor.error = None;
                        }
                        Err(error) => {
                            editor.error = Some(rom_folder_error_text(&self.i18n, error));
                        }
                    }
                }
                Task::none()
            }
            Message::EmulationFolderRemove(index) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    if index < editor.rom_folders.len() {
                        editor.rom_folders.remove(index);
                    }
                }
                Task::none()
            }
            Message::EmulationFolderRecursiveToggled(index) => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    if let Some(folder) = editor.rom_folders.get_mut(index) {
                        folder.recursive = !folder.recursive;
                    }
                }
                Task::none()
            }
            Message::EmulationProfileSave => {
                if self
                    .emulation
                    .editor
                    .as_ref()
                    .map(|editor| editor.saving)
                    .unwrap_or(true)
                {
                    return Task::none();
                }
                if !crate::backend::valid_emulator_executable(
                    &self
                        .emulation
                        .editor
                        .as_ref()
                        .map(|editor| editor.executable.as_str())
                        .unwrap_or_default(),
                ) {
                    let message = self.i18n.t("emulacao.apontar_exe_emulador").to_string();
                    if let Some(editor) = self.emulation.editor.as_mut() {
                        editor.error = Some(message);
                    }
                    return Task::none();
                }
                let Some(editor) = self.emulation.editor.as_ref() else {
                    return Task::none();
                };
                let executable = editor.executable.trim().to_string();
                let args = crate::backend::emulator_args_from_text(
                    &editor.args_text,
                    editor.original.as_ref(),
                );
                let core_path = editor.requires_core.then_some(editor.core_path.trim());
                let bios_path = editor.bios_required.then_some(editor.bios_path.trim());
                let payload = crate::backend::emulator_profile_payload(
                    &crate::backend::EmulatorProfileDraft {
                        id: &editor.emulator_id,
                        executable: &executable,
                        core_path,
                        bios_path,
                        rom_folders: &editor.rom_folders,
                        args: &args,
                        existing: editor.original.as_ref(),
                    },
                );
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.saving = true;
                    editor.confirm_remove = false;
                    editor.error = None;
                }
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.save_emulator_profile(payload).await },
                    Message::EmulationProfileSaved,
                )
            }
            Message::EmulationProfileSaved(result) => match result {
                Ok(()) => {
                    self.emulation.editor = None;
                    self.load_emulation(false)
                }
                Err(error) => {
                    if let Some(editor) = self.emulation.editor.as_mut() {
                        editor.saving = false;
                        editor.error = Some(error);
                    }
                    Task::none()
                }
            },
            Message::EmulationRemoveRequest => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    if editor.original.is_some() && !editor.saving {
                        editor.confirm_remove = true;
                        editor.error = None;
                    }
                }
                Task::none()
            }
            Message::EmulationRemoveCancel => {
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.confirm_remove = false;
                }
                Task::none()
            }
            Message::EmulationRemoveConfirm => {
                let Some(editor) = self.emulation.editor.as_ref() else {
                    return Task::none();
                };
                if editor.saving {
                    return Task::none();
                }
                let id = editor.emulator_id.clone();
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.saving = true;
                    editor.confirm_remove = false;
                    editor.error = None;
                }
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.remove_emulator_profile(id).await },
                    Message::EmulationProfileRemoved,
                )
            }
            Message::EmulationProfileRemoved(result) => match result {
                Ok(()) => {
                    self.emulation.editor = None;
                    self.load_emulation(false)
                }
                Err(error) => {
                    if let Some(editor) = self.emulation.editor.as_mut() {
                        editor.saving = false;
                        editor.error = Some(error);
                    }
                    Task::none()
                }
            },
            Message::EmulationScan => {
                let Some(editor) = self.emulation.editor.as_ref() else {
                    return Task::none();
                };
                if editor.scanning {
                    return Task::none();
                }
                let saved_folders = editor
                    .original
                    .as_ref()
                    .map(|profile| profile.rom_folders.as_slice())
                    .unwrap_or_default();
                if editor.rom_folders.is_empty() || editor.rom_folders.as_slice() != saved_folders {
                    let message = self.i18n.t("native.emulation.scan_need_save").to_string();
                    if let Some(editor) = self.emulation.editor.as_mut() {
                        editor.scan_error = Some(message);
                    }
                    return Task::none();
                }
                let id = editor.emulator_id.clone();
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.scanning = true;
                    editor.scan_error = None;
                }
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.scan_configured_emulator_roms(id).await },
                    Message::EmulationScanDone,
                )
            }
            Message::EmulationScanDone(result) => {
                let empty_message = self.i18n.t("emulacao.nenhuma_rom_pastas").to_string();
                match result {
                    Ok(scan) => {
                        if let Some(editor) = self.emulation.editor.as_mut() {
                            editor.scanning = false;
                            editor.scan_error = if scan.roms.is_empty() {
                                Some(empty_message)
                            } else {
                                None
                            };
                            editor.preview_truncated = scan.truncated;
                            editor.preview_available = true;
                            editor.preview_roms = scan.roms;
                        }
                    }
                    Err(error) => {
                        if let Some(editor) = self.emulation.editor.as_mut() {
                            editor.scanning = false;
                            editor.scan_error = Some(error);
                        }
                    }
                }
                Task::none()
            }
            Message::EmulationImportRom { path, name } => {
                let Some(editor) = self.emulation.editor.as_ref() else {
                    return Task::none();
                };
                if editor.importing_rom.is_some() {
                    return Task::none();
                }
                let emulator_id = editor.emulator_id.clone();
                let extra_args = crate::backend::emulator_args_from_text(
                    &editor.args_text,
                    editor.original.as_ref(),
                );
                let core_path = if editor.requires_core {
                    let core = editor.core_path.trim();
                    (!core.is_empty()).then(|| core.to_string())
                } else {
                    None
                };
                let occupied_ids = self
                    .library
                    .games
                    .iter()
                    .map(|game| game.id.clone())
                    .collect();
                let request = EmulatorRomImport {
                    emulator_id,
                    rom_path: path.clone(),
                    rom_name: name,
                    extra_args,
                    core_path,
                    occupied_ids,
                };
                if let Some(editor) = self.emulation.editor.as_mut() {
                    editor.importing_rom = Some(path);
                    editor.error = None;
                    editor.notice = None;
                }
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.import_emulator_rom(request).await },
                    |result| Message::EmulationImportDone { result },
                )
            }
            Message::EmulationImportDone { result } => match result {
                Ok(imported) => {
                    if let Some(editor) = self.emulation.editor.as_mut() {
                        editor.importing_rom = None;
                        editor.notice = Some(self.i18n.t_args(
                            "native.emulation.import_notice",
                            &[("title", &imported.title)],
                        ));
                    }
                    self.library.ready(imported.games);
                    self.big_picture
                        .sync_games(self.library.visible_all().len());
                    let urls = self.library.image_urls();
                    self.cover_tasks(urls)
                }
                Err(error) => {
                    if let Some(editor) = self.emulation.editor.as_mut() {
                        editor.importing_rom = None;
                        editor.error = Some(error);
                    }
                    Task::none()
                }
            },
            Message::SourcesLoaded(result) => {
                match result {
                    Ok(items) => {
                        self.sources.items = items;
                        self.sources.status = LoadStatus::Ready;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] fontes: {error}");
                        self.sources.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::SourceUrlChanged(url) => {
                self.sources.url_draft = url;
                Task::none()
            }
            Message::SourceAdd => {
                if self.sources.busy {
                    return Task::none();
                }
                let url = self.sources.url_draft.trim().to_string();
                if url.is_empty() {
                    self.sources.action_error =
                        Some(self.i18n.t("native.sources.url_required").to_string());
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.add_source(url).await },
                    Message::SourcesActionDone,
                )
            }
            Message::SourceRemove(id) => {
                if self.sources.busy {
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.remove_source(id).await },
                    Message::SourcesActionDone,
                )
            }
            Message::SourceSync => {
                if self.sources.busy {
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.sync_sources().await },
                    Message::SourcesActionDone,
                )
            }
            Message::SourcesActionDone(result) => {
                self.sources.busy = false;
                match result {
                    Ok(()) => {
                        self.sources.url_draft.clear();
                        self.sources.status = LoadStatus::Loading;
                        let backend = self.backend.clone();
                        Task::perform(
                            async move { backend.load_sources().await },
                            Message::SourcesLoaded,
                        )
                    }
                    Err(error) => {
                        self.sources.action_error = Some(source_error_text(&self.i18n, &error));
                        Task::none()
                    }
                }
            }
            Message::FriendsLoaded(result) => {
                match result {
                    Ok(data) => {
                        let urls = match &data {
                            FriendsLoad::Ready(data) => data.friends.iter().chain(&data.incoming).chain(&data.outgoing)
                                .filter_map(|friend|friend.avatar_url.clone()).collect(),
                            FriendsLoad::NotLoggedIn => Vec::new(),
                        };
                        self.friends.data = Some(data);
                        self.friends.status = LoadStatus::Ready;
                        let covers=self.cover_tasks(urls);
                        let avatars=self.ensure_friend_avatars();
                        return Task::batch([covers,avatars]);
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] amigos: {error}");
                        self.friends.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::HubcapKeyChanged(value) => {
                self.components.hubcap_draft = value;
                self.components.hubcap_test = None;
                Task::none()
            }
            Message::HubcapKeySave => {
                if self.components.hubcap_saving {
                    return Task::none();
                }
                let draft = self.components.hubcap_draft.trim().to_string();
                // A máscara que o config:get devolve não é uma chave nova.
                // Salvar "abc•••yz" apagaria a chave boa.
                if draft.contains('•') {
                    self.components.hubcap_notice =
                        Some(self.i18n.t("store_setup.hubcap_placeholder").to_string());
                    return Task::none();
                }
                self.components.hubcap_saving = true;
                self.components.hubcap_notice = None;
                self.components.hubcap_test = None;
                self.components.hubcap_secret = (!draft.is_empty()).then(|| draft.clone());
                let value = if draft.is_empty() {
                    Value::Null
                } else {
                    Value::String(draft.clone())
                };
                let save = self.save_setting("hubcap_api_key", value);
                let backend = self.backend.clone();
                let stats_key = draft.clone();
                let check = Task::perform(
                    async move { backend.validate_hubcap_key(draft).await },
                    |(ok, motivo)| Message::HubcapKeyChecked { ok, motivo },
                );
                let stats_backend = self.backend.clone();
                let stats = Task::perform(
                    async move { stats_backend.hubcap_stats(stats_key).await },
                    Message::HubcapStatsLoaded,
                );
                Task::batch([save, check, stats])
            }
            Message::HubcapKeyValidate => {
                if self.components.hubcap_validating {
                    return Task::none();
                }
                let draft = self.components.hubcap_draft.trim().to_string();
                if draft.contains('•') || draft.is_empty() {
                    self.components.hubcap_test = Some(if draft.is_empty() {
                        "sem_chave".to_string()
                    } else {
                        "indisponivel".to_string()
                    });
                    return Task::none();
                }
                self.components.hubcap_validating = true;
                self.components.hubcap_test = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.validate_hubcap_key(draft).await },
                    |(ok, motivo)| Message::HubcapKeyChecked { ok, motivo },
                )
            }
            Message::HubcapStatsLoaded(stats) => {
                self.components.hubcap_stats = Some(stats);
                Task::none()
            }
            Message::HubcapKeyChecked { ok, motivo } => {
                self.components.hubcap_saving = false;
                self.components.hubcap_validating = false;
                self.components.hubcap_test = Some(if ok {
                    "ok".to_string()
                } else {
                    motivo.unwrap_or_else(|| "indisponivel".to_string())
                });
                if ok {
                    self.components.hubcap_notice = Some(self.i18n.t("common.salvo").to_string());
                }
                Task::none()
            }
            Message::ComponentsLoaded(result) => {
                match result {
                    Ok(plugins) => {
                        self.components.plugins = plugins;
                        self.components.status = LoadStatus::Ready;
                        if self.components.hubcap_draft.is_empty() {
                            if let Some(key) = self
                                .settings
                                .config
                                .as_ref()
                                .map(|config| config.hubcap_api_key.clone())
                                .filter(|key| !key.is_empty())
                            {
                                self.components.hubcap_draft = key;
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] componentes: {error}");
                        self.components.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::PluginActionRequested { id, action } => {
                if self.components.busy.is_some() || self.components.registering {
                    return Task::none();
                }
                self.components.busy = Some(id.clone());
                self.components.banner = None;
                self.components.notices.remove(&id);
                let backend = self.backend.clone();
                let message_id = id.clone();
                Task::perform(
                    async move { backend.run_plugin_action(id, action).await },
                    move |result| Message::PluginActionDone {
                        id: message_id,
                        action,
                        result,
                    },
                )
            }
            Message::PluginActionDone { id, action, result } => {
                self.components.busy = None;
                match result {
                    Ok(PluginActionOutcome::Success) => {
                        self.components.notices.insert(
                            id,
                            PluginNotice {
                                ok: true,
                                message: action.success_message_i18n(&self.i18n).to_string(),
                            },
                        );
                        self.reload_components()
                    }
                    Ok(PluginActionOutcome::NotDetected) => {
                        let message = self.i18n.t_args(
                            "native.components.action_not_detected",
                            &[("action", action.label_i18n(&self.i18n))],
                        );
                        self.components.notices.insert(
                            id,
                            PluginNotice {
                                ok: false,
                                message: message.clone(),
                            },
                        );
                        self.components.banner = Some(PluginNotice { ok: false, message });
                        self.reload_components()
                    }
                    Err(error) => {
                        let message = self.i18n.t_args(
                            "native.components.action_failed",
                            &[("action", action.label_i18n(&self.i18n)), ("error", &error)],
                        );
                        self.components.notices.insert(
                            id,
                            PluginNotice {
                                ok: false,
                                message: message.clone(),
                            },
                        );
                        self.components.banner = Some(PluginNotice { ok: false, message });
                        self.reload_components()
                    }
                }
            }
            Message::PluginVerifyRequested(id) => {
                if self.components.busy.is_some() || self.components.registering {
                    return Task::none();
                }
                self.components.busy = Some(id.clone());
                self.components.banner = None;
                self.components.notices.remove(&id);
                let backend = self.backend.clone();
                let message_id = id.clone();
                Task::perform(
                    async move { backend.verify_plugin(id).await },
                    move |result| Message::PluginVerifyDone {
                        id: message_id,
                        result,
                    },
                )
            }
            Message::PluginVerifyDone { id, result } => {
                self.components.busy = None;
                match result {
                    Ok(verification) => {
                        let ok = verification.ok && verification.valid;
                        self.components.notices.insert(
                            id,
                            PluginNotice {
                                ok,
                                message: verification_summary(&self.i18n, &verification),
                            },
                        );
                    }
                    Err(error) => {
                        self.components.notices.insert(
                            id,
                            PluginNotice {
                                ok: false,
                                message: error.clone(),
                            },
                        );
                        self.components.banner = Some(PluginNotice {
                            ok: false,
                            message: self
                                .i18n
                                .t_args("native.components.verify_failed", &[("error", &error)]),
                        });
                    }
                }
                self.reload_components()
            }
            Message::PluginDetailsRequested(id) => {
                self.components.details = Some(PluginDetailsPanel {
                    id: id.clone(),
                    status: LoadStatus::Loading,
                    details: None,
                    error: None,
                });
                let backend = self.backend.clone();
                let message_id = id.clone();
                Task::perform(
                    async move { backend.load_plugin_details(id).await },
                    move |result| Message::PluginDetailsDone {
                        id: message_id,
                        result,
                    },
                )
            }
            Message::PluginDetailsDone { id, result } => {
                let Some(panel) = self.components.details.as_mut() else {
                    return Task::none();
                };
                if panel.id != id {
                    return Task::none();
                }
                match result {
                    Ok(details) => {
                        panel.details = Some(details);
                        panel.status = LoadStatus::Ready;
                        panel.error = None;
                    }
                    Err(error) => {
                        panel.details = None;
                        panel.status = LoadStatus::Error(error.clone());
                        panel.error = Some(error);
                    }
                }
                Task::none()
            }
            Message::PluginDetailsClose => {
                self.components.details = None;
                Task::none()
            }
            Message::PluginPathChanged(path) => {
                self.components.path_draft = path;
                Task::none()
            }
            Message::PluginRegisterRequested => {
                if self.components.registering || self.components.busy.is_some() {
                    return Task::none();
                }
                let path = self.components.path_draft.trim().to_string();
                if path.is_empty() {
                    self.components.banner = Some(PluginNotice {
                        ok: false,
                        message: self.i18n.t("native.components.path_required").to_string(),
                    });
                    return Task::none();
                }
                self.components.registering = true;
                self.components.banner = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.register_plugin(path).await },
                    Message::PluginRegisterDone,
                )
            }
            Message::PluginRegisterDone(result) => {
                self.components.registering = false;
                match result {
                    Ok(registration) => {
                        self.components.path_draft.clear();
                        let message = if registration.name.is_empty()
                            || registration.name == registration.id
                        {
                            self.i18n
                                .t_args("native.components.registered", &[("id", &registration.id)])
                        } else {
                            self.i18n.t_args(
                                "native.components.registered_named",
                                &[("name", &registration.name), ("id", &registration.id)],
                            )
                        };
                        self.components.banner = Some(PluginNotice { ok: true, message });
                        self.reload_components()
                    }
                    Err(error) => {
                        self.components.banner = Some(PluginNotice {
                            ok: false,
                            message: self
                                .i18n
                                .t_args("native.components.register_failed", &[("error", &error)]),
                        });
                        Task::none()
                    }
                }
            }
            Message::CommunityTabChanged(tab) => {
                if let Some(appid) = self.current_community_appid() {
                    if let Some(state) = self.community.get_mut(&appid) {
                        state.tab = tab;
                    }
                }
                Task::none()
            }
            Message::CommunityRefresh(tab) => {
                let Some(appid) = self.current_community_appid() else {
                    return Task::none();
                };
                match tab {
                    CommunityTab::Reviews => self.load_community_reviews(&appid, 0, false),
                    CommunityTab::Collections => self.load_community_collections(&appid, 0, false),
                }
            }
            Message::CommunityLoadMore(tab) => {
                let Some(appid) = self.current_community_appid() else {
                    return Task::none();
                };
                let Some(state) = self.community.get(&appid) else {
                    return Task::none();
                };
                let offset = match tab {
                    CommunityTab::Reviews => state.reviews.next_offset,
                    CommunityTab::Collections => state.collections.next_offset,
                };
                match tab {
                    CommunityTab::Reviews => self.load_community_reviews(&appid, offset, true),
                    CommunityTab::Collections => {
                        self.load_community_collections(&appid, offset, true)
                    }
                }
            }
            Message::CommunityReviewsLoaded {
                appid,
                offset,
                append,
                result,
            } => {
                let Some(state) = self.community.get_mut(&appid) else {
                    return Task::none();
                };
                state.reviews.loading_more = false;
                match result {
                    Ok(page) => state.apply_reviews_page(page, offset, append),
                    Err(error) => {
                        if append && !state.reviews.items.is_empty() {
                            state.reviews.error = Some(error.message);
                            state.reviews.offline = error.offline;
                        } else {
                            state.reviews.status = LoadStatus::Error(error.message.clone());
                            state.reviews.error = Some(error.message);
                            state.reviews.offline = error.offline;
                            state.reviews.items.clear();
                            state.reviews.has_more = false;
                            state.reviews.next_offset = 0;
                        }
                    }
                }
                Task::none()
            }
            Message::CommunityCollectionsLoaded {
                appid,
                offset,
                append,
                result,
            } => {
                let Some(state) = self.community.get_mut(&appid) else {
                    return Task::none();
                };
                state.collections.loading_more = false;
                match result {
                    Ok(page) => state.apply_collections_page(page, offset, append),
                    Err(error) => {
                        if append && !state.collections.items.is_empty() {
                            state.collections.error = Some(error.message);
                            state.collections.offline = error.offline;
                        } else {
                            state.collections.status = LoadStatus::Error(error.message.clone());
                            state.collections.error = Some(error.message);
                            state.collections.offline = error.offline;
                            state.collections.items.clear();
                            state.collections.has_more = false;
                            state.collections.next_offset = 0;
                        }
                    }
                }
                Task::none()
            }
            Message::CommunityReviewFormOpened => self.open_community_review_form(None),
            Message::CommunityReviewEditRequested(id) => self.open_community_review_form(Some(id)),
            Message::CommunityReviewFormClosed => {
                if let Some(state) = self.current_community_state_mut() {
                    state.review_form = None;
                }
                Task::none()
            }
            Message::CommunityReviewTitleChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.review_form.as_mut())
                {
                    form.title = value;
                }
                Task::none()
            }
            Message::CommunityReviewTextChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.review_form.as_mut())
                {
                    form.text = value;
                }
                Task::none()
            }
            Message::CommunityReviewRatingChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.review_form.as_mut())
                {
                    form.rating = value;
                }
                Task::none()
            }
            Message::CommunityReviewHoursChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.review_form.as_mut())
                {
                    form.hours = value;
                }
                Task::none()
            }
            Message::CommunityReviewSubmit => self.submit_community_review(),
            Message::CommunityCollectionFormOpened => self.open_community_collection_form(None),
            Message::CommunityCollectionEditRequested(id) => {
                self.open_community_collection_form(Some(id))
            }
            Message::CommunityCollectionFormClosed => {
                if let Some(state) = self.current_community_state_mut() {
                    state.collection_form = None;
                }
                Task::none()
            }
            Message::CommunityCollectionTitleChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.collection_form.as_mut())
                {
                    form.title = value;
                }
                Task::none()
            }
            Message::CommunityCollectionDescriptionChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.collection_form.as_mut())
                {
                    form.description = value;
                }
                Task::none()
            }
            Message::CommunityCollectionVisibilityChanged(value) => {
                if let Some(form) = self
                    .current_community_state_mut()
                    .and_then(|state| state.collection_form.as_mut())
                {
                    form.visibility = value;
                }
                Task::none()
            }
            Message::CommunityCollectionSubmit => self.submit_community_collection(),
            Message::CommunityMineToggled(value) => self.toggle_community_mine(value),
            Message::CommunityCollectionItemAdd(id) => {
                self.start_community_item(CommunityMutationKind::CollectionItemAdd, id)
            }
            Message::CommunityCollectionItemRemove(id) => {
                self.start_community_item(CommunityMutationKind::CollectionItemRemove, id)
            }
            Message::CommunityConfirmRequested(target, id) => {
                if let Some(state) = self.current_community_state_mut() {
                    state.confirm = Some(match target {
                        CommunityTarget::Review => CommunityConfirm::Review(id),
                        CommunityTarget::Collection => CommunityConfirm::Collection(id),
                    });
                    state.report = None;
                    state.mutation_error = None;
                    state.mutation_notice = None;
                }
                Task::none()
            }
            Message::CommunityConfirmCancelled => {
                if let Some(state) = self.current_community_state_mut() {
                    state.confirm = None;
                }
                Task::none()
            }
            Message::CommunityConfirmAccepted => self.confirm_community_removal(),
            Message::CommunityReportRequested(target, id) => {
                let reason = self.i18n.t("community.conteudo_inadequado").to_string();
                if let Some(state) = self.current_community_state_mut() {
                    state.report = Some(CommunityReportState { target, id, reason });
                    state.confirm = None;
                    state.mutation_error = None;
                    state.mutation_notice = None;
                }
                Task::none()
            }
            Message::CommunityReportReasonChanged(value) => {
                if let Some(report) = self
                    .current_community_state_mut()
                    .and_then(|state| state.report.as_mut())
                {
                    report.reason = value;
                }
                Task::none()
            }
            Message::CommunityReportCancelled => {
                if let Some(state) = self.current_community_state_mut() {
                    state.report = None;
                }
                Task::none()
            }
            Message::CommunityReportSubmit => self.submit_community_report(),
            Message::CommunityMutationDone {
                appid,
                action,
                result,
            } => self.finish_community_mutation(appid, action, result),
            Message::StoreTabChanged(tab) => {
                if self.store.tab == tab {
                    return Task::none();
                }
                self.store_thumbnail_queue.clear();
                self.store.tab = tab;
                self.store.detail = None;
                self.retro.detail = None;
                let refresh = self.refresh_visible_section();
                let covers = if tab == StoreTab::Retro {
                    self.retro_page_cover_tasks(RETRO_PAGE_SIZE as usize)
                } else {
                    Task::none()
                };
                Task::batch([refresh, covers])
            }
            Message::StoreQueryChanged(query) => {
                self.store.query = query;
                let trimmed = self.store.query.trim();
                if trimmed.chars().count() < 2 {
                    self.store.suggestions.clear();
                    self.store.suggested_query.clear();
                    return Task::none();
                }
                if self.store.suggested_query == trimmed {
                    return Task::none();
                }
                self.store.suggested_query = trimmed.to_string();
                let request = trimmed.to_string();
                let message_request = request.clone();
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.suggest_store(request).await },
                    move |result| Message::StoreSuggestDone {
                        query: message_request,
                        result,
                    },
                )
            }
            Message::StoreSearchSubmitted => self.submit_store_search(),
            Message::StoreSuggestionPicked(suggestion) => {
                self.store.query = suggestion.title;
                self.store.suggestions.clear();
                self.submit_store_search()
            }
            Message::StoreClearSearch => {
                if self.store.search.is_some() {
                    self.store_thumbnail_queue.clear();
                }
                self.store.query.clear();
                self.store.suggestions.clear();
                self.store.suggested_query.clear();
                self.store.search = None;
                self.sync_console_store();
                Task::none()
            }
            Message::StoreSearchDone { query, result } => {
                let Some(search) = self.store.search.as_mut() else {
                    return Task::none();
                };
                if search.query != query {
                    return Task::none();
                }
                match result {
                    Ok(found) => {
                        eprintln!(
                            "[arcadia-native] loja: {} resultado(s) para {query:?}",
                            found.games.len()
                        );
                        search.result = Some(found);
                        search.status = LoadStatus::Ready;
                        search.error = None;
                        self.sync_console_store();
                        if self.console_store_visible() {
                            self.prefetch_store_front()
                        } else {
                            Task::none()
                        }
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja: {error}");
                        search.result = None;
                        search.status = LoadStatus::Error(error.clone());
                        search.error = Some(error);
                        self.sync_console_store();
                        Task::none()
                    }
                }
            }
            Message::StoreSuggestDone { query, result } => {
                if self.store.query.trim() != query {
                    return Task::none();
                }
                // Sugestão em voo do termo que acabou de virar busca não deve
                // reabrir a lista sobre os resultados.
                if self
                    .store
                    .search
                    .as_ref()
                    .is_some_and(|search| search.query == query)
                {
                    return Task::none();
                }
                match result {
                    Ok(games) => {
                        self.store.suggestions = games;
                        Task::none()
                    }
                    Err(_) => {
                        // Sugestão é auxiliar: uma falha aqui não esconde a busca.
                        self.store.suggestions.clear();
                        Task::none()
                    }
                }
            }
            Message::StoreBrowseLoaded { offset, generation, result } => {
                if generation != self.store.browse.generation {
                    return Task::none();
                }
                let append = offset != 0;
                if append && !self.store.browse.loading_more {
                    self.store.browse.next_page = None;
                    if let Ok(page) = result {
                        let urls = store_thumbnail_urls(&page.games);
                        self.store.browse.next_page = Some(NextStorePage::Ready(page));
                        if self.desktop_store_visible() {
                            return self.store_thumbnail_tasks(urls);
                        }
                    }
                    return Task::none();
                }
                self.store.browse.next_page = None;
                self.store.browse.loading_more = false;
                match result {
                    Ok(page) => {
                        let urls = store_thumbnail_urls(&page.games);
                        eprintln!(
                            "[arcadia-native] loja: {} jogo(s) no catálogo (total {:?})",
                            page.games.len(),
                            page.total
                        );
                        if append {
                            for game in page.games {
                                if !self
                                    .store
                                    .browse
                                    .games
                                    .iter()
                                    .any(|current| current.appid == game.appid)
                                {
                                    self.store.browse.games.push(game);
                                }
                            }
                            if let Some(total) = page.total {
                                self.store.browse.total = Some(total);
                            }
                        } else {
                            self.store.browse.games = page.games;
                            self.store.browse.total = page.total;
                        }
                        self.store.browse.has_more = page.has_more;
                        self.store.browse.next_offset = page.next_offset.or_else(|| {
                            page.has_more
                                .then_some(self.store.browse.games.len() as u64)
                        });
                        self.store.browse.status = LoadStatus::Ready;
                        self.store.browse.error = None;
                        self.sync_console_store();
                        if self.console_store_visible() {
                            self.prefetch_store_front()
                        } else if !append || self.desktop_store_visible() {
                            self.store_thumbnail_tasks(urls)
                        } else {
                            Task::none()
                        }
                    }
                    Err(error) => {
                        if append {
                            self.store.browse.error = Some(error);
                        } else {
                            eprintln!("[arcadia-native] loja: {error}");
                            self.store.browse.status = if !self.store.browse.games.is_empty() {
                                LoadStatus::Ready
                            } else if self.desktop_store_visible() || self.console_store_visible()
                            {
                                LoadStatus::Error(error.clone())
                            } else {
                                LoadStatus::Idle
                            };
                            self.store.browse.error = Some(error);

                        }
                        self.sync_console_store();
                        Task::none()
                    }
                }
            }
            Message::StorePrefetchNext => {
                if self.desktop_store_visible() && self.store.search.is_none() {
                    self.request_next_store_page(false)
                } else {
                    Task::none()
                }
            }
            Message::StoreLoadMore => self.load_store_more(),
            Message::PrefetchGame(id) => self.prefetch_game(id),
            Message::PrefetchStore(game) => self.prefetch_store(game),
            Message::PrefetchedStore { appid, result } => {
                self.detail_prefetching.remove(&format!("store:{appid}"));
                let Ok(info) = result else { return self.prefetch_store_genres(); };
                let urls = if self.console_store_visible() {
                    let mut urls = info.image_urls();
                    urls.truncate(8);
                    urls.extend(info.description_image_urls().into_iter().take(3));
                    urls
                } else {
                    Vec::new()
                };
                self.cache_store_info(&appid, info);
                self.sync_console_store();
                let covers = self.cover_tasks(urls);
                Task::batch([covers, self.prefetch_store_genres()])
            }
            Message::PrefetchedGame { id, media } => {
                self.detail_prefetching.remove(&format!("game:{id}"));
                let urls = media.movies.iter().filter_map(|m| m.thumb.clone())
                    .chain(media.screenshots.iter().filter_map(|s| s.thumb_or_full().map(str::to_string)))
                    .take(6).collect();
                self.cache_game_media(&id, media);
                self.cover_tasks(urls)
            }
            Message::StoreOpenDetail(game) => self.open_store_detail(game),
            Message::StoreCloseDetail => {
                self.store.detail = None;
                self.animated_cards.clear_cards();
                self.card_videos.clear();
                self.page_entered = Instant::now();
                self.page_direction = -1.0;
                if matches!(self.screen, Screen::Section(Section::Home)) {
                    self.home_entered = Instant::now();
                    self.home_hero_started = Instant::now();
                }
                self.media.clear();
                self.game_media = None;
                self.gallery_trailer = false;
                Task::none()
            }
            Message::StoreScreenshotSelected(index) => {
                if let Some(detail) = self.store.detail.as_mut() {
                    detail.selected_screenshot = index;
                }
                self.gallery_trailer = false;
                let _ = self.media.pause();
                let urls = self.store.detail.as_ref().and_then(|detail| detail.info.as_ref())
                    .map(|info| info.screenshots.iter().skip(index).take(2)
                        .filter_map(|shot| shot.full_or_thumb().map(str::to_string)).collect()).unwrap_or_default();
                self.cover_tasks(urls)
            }
            Message::StoreSelectTrailer => {
                self.gallery_trailer = true;
                if self.media.is_active() && self.media.play().is_ok() {
                    return Task::none();
                }
                let sources = self
                    .store
                    .detail
                    .as_ref()
                    .and_then(|detail| detail.info.as_ref())
                    .and_then(|info| info.movies.first())
                    .map(|movie| media::TrailerSources {
                        hls: movie.hls.clone(),
                        mp4: movie.mp4.clone(),
                        webm: movie.webm.clone(),
                    });
                match sources {
                    Some(sources) => match self.media.open(&sources) {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::MediaError(error)),
                    },
                    None => Task::none(),
                }
            }
            Message::HomeLoaded(result) => {
                self.home.hero_status = LoadStatus::Loading;
                match result {
                    Ok(page) => {
                        let urls: Vec<String> = page
                            .games
                            .iter()
                            .flat_map(StoreGame::image_urls)
                            .chain(page.games.iter().filter_map(crate::backend::store_hero_url))
                            .collect();
                        eprintln!(
                            "[arcadia-native] home: {} jogo(s) em alta ({} ms)",
                            page.games.len(),
                            super::timing::elapsed_ms("home")
                        );
                        self.home_entered = Instant::now();
                        self.home_hero_started = Instant::now();
                        self.home_hero_index = 0;

                        self.home.games = page.games;
                        self.home.status = LoadStatus::Ready;
                        self.home.error = None;
                        let cover_task = self.cover_tasks(urls);
                        let hero_game = self.home.games.first().cloned();
                        let hero_task = match hero_game {
                            Some(game) => self.home_hero_info_task(&game),
                            None => {
                                self.home.hero_status = LoadStatus::Ready;
                                Task::none()
                            }
                        };
                        Task::batch([cover_task, hero_task])
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] home: {error}");
                        self.home.status = LoadStatus::Error(error.clone());
                        self.home.error = Some(error.clone());
                        self.home.hero_status = LoadStatus::Error(error);
                        Task::none()
                    }
                }
            }
            Message::HomeHeroInfoLoaded { appid, result } => {
                if let Ok(info) = &result {
                    self.cache_store_info(&appid, info.clone());
                }
                if self.home.hero_appid.as_deref() != Some(appid.as_str()) {
                    return Task::none();
                }
                match result {
                    Ok(info) => {
                        let urls = info.image_urls();
                        self.home.hero_info = Some(info);
                        self.home.hero_status = LoadStatus::Ready;
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] home (herói): {error}");
                        self.home.hero_info = None;
                        self.home.hero_status = LoadStatus::Error(error.clone());
                        Task::none()
                    }
                }
            }
            Message::HomeRollDice => {
                if self.home.games.is_empty() {
                    return Task::none();
                }
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.subsec_nanos() as usize)
                    .unwrap_or(0);
                let index = nanos % self.home.games.len();
                let game = self.home.games[index].clone();
                self.open_store_detail(game)
            }
            Message::StoreDetailLoaded { appid, result } => {
                if let Ok(info) = &result {
                    self.cache_store_info(&appid, info.clone());
                }
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                match result {
                    Ok(info) => {
                        let mut urls = info.image_urls();
                        urls.extend(info.description_image_urls());
                        let about = info.about.clone();
                        let sources = info.movies.first().map(|movie| media::TrailerSources {
                            hls: movie.hls.clone(),
                            mp4: movie.mp4.clone(),
                            webm: movie.webm.clone(),
                        });
                        detail.info = Some(info);
                        detail.status = LoadStatus::Ready;
                        detail.error = None;
                        self.gallery_trailer = sources.is_some();
                        self.register_description_videos(about.as_deref());
                        let covers = self.cover_tasks(urls);
                        let playback = match sources {
                            Some(sources) => match self.media.open(&sources) {
                                Ok(()) => Task::none(),
                                Err(error) => Task::done(Message::MediaError(error)),
                            },
                            None => Task::none(),
                        };
                        Task::batch([covers, playback, views::game_detail::scroll_to_top()])
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (ficha): {error}");
                        detail.status = LoadStatus::Error(error.clone());
                        detail.error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::StoreInstallInfoLoaded { appid, result } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                match result {
                    Ok(info) => {
                        detail.selected_depots = info.recommended_depots();
                        detail.install = Some(info);
                        detail.install_status = LoadStatus::Ready;
                        detail.install_error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (installInfo): {error}");
                        detail.install = None;
                        detail.install_status = LoadStatus::Error(error.clone());
                        detail.install_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::StoreSourceOffersLoaded { appid, generation, cached_only, result } => {
                let Some(detail) = self.store.detail.as_mut().filter(|detail| {
                    detail.game.appid == appid && detail.downloads.generation == generation
                }) else { return Task::none() };
                if cached_only && detail.downloads.status != LoadStatus::Loading {
                    return Task::none();
                }
                match result {
                    Ok(mut data) => {
                        let selection = detail.downloads.data.offers.get(detail.downloads.selected_offer)
                            .map(|offer| (offer.reference.clone(), offer.uris.get(detail.downloads.selected_uri).cloned()));
                        if cached_only { data.unavailable_sources.clear(); }
                        detail.downloads.data = data;
                        if !cached_only { detail.downloads.status = LoadStatus::Ready; }
                        detail.downloads.selected_offer = 0;
                        detail.downloads.selected_uri = 0;
                        if let Some((reference, uri)) = selection {
                            if let Some(index) = detail.downloads.data.offers.iter().position(|offer| offer.reference == reference) {
                                detail.downloads.selected_offer = index;
                                if let Some(uri) = uri {
                                    detail.downloads.selected_uri = detail.downloads.data.offers[index].uris.iter().position(|candidate| candidate == &uri).unwrap_or(0);
                                }
                            }
                        }
                    }
                    Err(error) => {
                        detail.downloads.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::StoreSourcesRetry => self.store_source_offers_task(),
            Message::StoreDownloadRequested => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if !detail.install_busy {
                        detail.downloads.open = true;
                    }
                }
                Task::none()
            }
            Message::StoreDownloadClosed => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if !detail.install_busy && !detail.downloads.pick_busy {
                        detail.downloads.open = false;
                    }
                }
                Task::none()
            }
            Message::StoreDownloadKey(event) => {
                if matches!(event, keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. }) {
                    return self.update(Message::StoreDownloadClosed);
                }
                Task::none()
            }
            Message::StoreSourceSelected(reference) => {
                if let Some(detail) = self.store.detail.as_mut().filter(|detail| !detail.install_busy) {
                    if let Some(index) = detail.downloads.data.offers.iter().position(|offer| offer.reference == reference) {
                        detail.downloads.selected_offer = index;
                        detail.downloads.selected_uri = 0;
                        detail.downloads.error = None;
                    }
                }
                Task::none()
            }
            Message::StoreSourceUriSelected(index) => {
                if let Some(detail) = self.store.detail.as_mut().filter(|detail| !detail.install_busy) {
                    if detail.downloads.data.offers.get(detail.downloads.selected_offer).is_some_and(|offer| index < offer.uris.len()) {
                        detail.downloads.selected_uri = index;
                    }
                }
                Task::none()
            }
            Message::StoreDownloadPathChanged(path) => {
                if let Some(detail) = self.store.detail.as_mut().filter(|detail| !detail.install_busy) {
                    detail.downloads.path = path;
                }
                Task::none()
            }
            Message::StoreSourceDownloadConfirmed => {
                let Some(detail) = self.store.detail.as_mut() else { return Task::none() };
                if detail.install_busy || detail.downloads.pick_busy || !detail.downloads.open {
                    return Task::none();
                }
                let Some(offer) = detail.downloads.data.offers.get(detail.downloads.selected_offer) else { return Task::none() };
                let Some(payload) = crate::backend::source_download_payload(&detail.game, offer, detail.downloads.selected_uri, &detail.downloads.path) else {
                    return Task::none();
                };
                detail.install_busy = true;
                detail.downloads.error = None;
                detail.install_message = None;
                let appid = detail.game.appid.clone();
                let generation = detail.downloads.generation;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.download_source_offer(payload).await },
                    move |result| Message::StoreSourceDownloadDone { appid, generation, result },
                )
            }
            Message::StoreSourceDownloadDone { appid, generation, result } => {
                let Some(detail) = self.store.detail.as_mut().filter(|detail| {
                    detail.game.appid == appid && detail.downloads.generation == generation
                }) else { return Task::none() };
                detail.install_busy = false;
                match result {
                    Ok(()) => {
                        detail.downloads.open = false;
                        detail.downloads.error = None;
                        detail.install_message = Some(self.i18n.t_args("native.store.queued", &[("title", &detail.game.title)]));
                    }
                    Err(error) => detail.downloads.error = Some(error),
                }
                Task::none()
            }
            Message::StoreLibrariesLoaded(result) => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                match result {
                    Ok(libraries) => {
                        detail.libraries = libraries;
                        if detail.selected_library >= detail.libraries.len() {
                            detail.selected_library = 0;
                        }
                        detail.libraries_status = LoadStatus::Ready;
                        detail.libraries_error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (libraries): {error}");
                        detail.libraries.clear();
                        detail.libraries_status = LoadStatus::Error(error.clone());
                        detail.libraries_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::StoreDepotToggled(depot_id) => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if !detail.selected_depots.remove(&depot_id) {
                        detail.selected_depots.insert(depot_id);
                    }
                    detail.install_message = None;
                }
                Task::none()
            }
            Message::StoreRecommendedDepots => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if let Some(info) = detail.install.as_ref() {
                        detail.selected_depots = info.recommended_depots();
                        detail.install_message = None;
                    }
                }
                Task::none()
            }
            Message::StoreClearDepots => {
                if let Some(detail) = self.store.detail.as_mut() {
                    detail.selected_depots.clear();
                    detail.install_message = None;
                }
                Task::none()
            }
            Message::StoreLibrarySelected(index) => {
                if let Some(detail) = self.store.detail.as_mut() {
                    if index < detail.libraries.len() {
                        detail.selected_library = index;
                        detail.install_message = None;
                    }
                }
                Task::none()
            }
            Message::StoreAddToLibrary => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.install_busy {
                    return Task::none();
                }
                let game = detail.game.clone();
                let info = detail.install.clone();
                detail.install_busy = true;
                detail.install_error = None;
                detail.install_message = None;
                let backend = self.backend.clone();
                let appid = game.appid.clone();
                Task::perform(
                    async move {
                        // Salva no Arcadia antes de iniciar a integração Steam.
                        let games = backend.add_store_game_to_library(&game).await?;
                        let integration = backend.library_integration_enabled().await;
                        let mut injection_failed = None;
                        if integration {
                            match backend.add_store_game_to_steam(&game, info.as_ref()).await {
                                Ok(()) => {}
                                Err(Some(plugin)) if plugin == "slssteam" => {}
                                Err(reason) => {
                                    injection_failed = Some(reason.unwrap_or_default());
                                }
                            }
                        }
                        Ok((games, injection_failed))
                    },
                    move |result| match result {
                        Ok((games, injection_failed)) => Message::StoreAddedToLibrary {
                            appid,
                            result: Ok(games),
                            injection_failed,
                        },
                        Err(error) => Message::StoreAddedToLibrary {
                            appid,
                            result: Err(error),
                            injection_failed: None,
                        },
                    },
                )
            }
            Message::StoreRemoveFromLibrary => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.install_busy {
                    return Task::none();
                }
                let appid = detail.game.appid.clone();
                detail.install_busy = true;
                detail.install_error = None;
                detail.install_message = None;
                let backend = self.backend.clone();
                let request = appid.clone();
                Task::perform(
                    async move { backend.remove_store_game_from_library(request).await },
                    move |result| Message::StoreRemovedFromLibrary { appid, result },
                )
            }
            Message::StoreRemovedFromLibrary { appid, result } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                detail.install_busy = false;
                match result {
                    Ok(games) => {
                        let title = detail.game.title.clone();
                        eprintln!("[arcadia-native] loja: {title} saiu da biblioteca");
                        self.library.ready(games);
                        detail.install_error = None;
                        detail.install_message = Some(self.i18n.t_args(
                            "native.store.removed",
                            &[("title", &title)],
                        ));
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (removeFromLibrary): {error}");
                        detail.install_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::StoreAddedToLibrary {
                appid,
                result,
                injection_failed,
            } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                detail.install_busy = false;
                match result {
                    Ok(games) => {
                        let title = detail.game.title.clone();
                        eprintln!("[arcadia-native] loja: {title} entrou na biblioteca");
                        self.library.ready(games);
                        detail.install_error = None;
                        detail.install_message = Some(match injection_failed {
                            Some(reason) => self.i18n.t_args(
                                "store.adicionado_sem_injecao",
                                &[("titulo", &title), ("motivo", &reason)],
                            ),
                            None => self.i18n.t_args(
                                "store.adicionado_biblioteca",
                                &[("titulo", &title)],
                            ),
                        });
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (addToLibrary): {error}");
                        detail.install_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::StoreInstallRequested => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.install_busy {
                    return Task::none();
                }
                let Some(info) = detail.install.as_ref() else {
                    detail.install_error = Some(
                        self.i18n
                            .t("native.store.install_manifest_not_loaded")
                            .to_string(),
                    );
                    return Task::none();
                };
                let Some(library) = detail.libraries.get(detail.selected_library) else {
                    detail.install_error = Some(
                        self.i18n
                            .t("native.store.install_no_steam_library")
                            .to_string(),
                    );
                    return Task::none();
                };
                let depots: Vec<StoreDepot> = info
                    .depots
                    .iter()
                    .filter(|depot| detail.selected_depots.contains(&depot.depot_id))
                    .cloned()
                    .collect();
                if depots.is_empty() {
                    detail.install_error =
                        Some(self.i18n.t("native.store.install_select_depot").to_string());
                    return Task::none();
                }

                let payload = crate::backend::store_install_payload(
                    &detail.game,
                    info,
                    &library.steam_dir,
                    &depots,
                );
                let appid = detail.game.appid.clone();
                detail.install_busy = true;
                detail.install_error = None;
                detail.install_message = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.install_store_game(payload).await },
                    move |result| Message::StoreInstallDone { appid, result },
                )
            }
            Message::StoreInstallDone { appid, result } => {
                let Some(detail) = self.store.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.appid != appid {
                    return Task::none();
                }
                detail.install_busy = false;
                match result {
                    Ok(()) => {
                        detail.downloads.open = false;
                        eprintln!(
                            "[arcadia-native] loja: {} enfileirado em store:install",
                            detail.game.title
                        );
                        detail.install_error = None;
                        detail.install_message = Some(
                            self.i18n
                                .t_args("native.store.queued", &[("title", &detail.game.title)]),
                        );
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] loja (install): {error}");
                        detail.install_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::RetroQueryChanged(query) => {
                self.retro.query = query;
                Task::none()
            }
            Message::RetroSystemChanged(system) => {
                if self.retro.system == system {
                    return Task::none();
                }
                self.retro.system = system;
                self.load_retro_page(0)
            }
            Message::RetroSearchSubmitted => self.submit_retro_search(),
            Message::RetroClearSearch => {
                self.retro.query.clear();
                self.retro.applied_query.clear();
                self.load_retro_page(0)
            }
            Message::RetroPageCached { generation, offset, payload, page } => {
                if generation != self.retro.generation {
                    return Task::none();
                }
                let covers = match page {
                    Some(mut page) if !self.retro.showing_cached_page => {
                        page.offset = offset;
                        self.retro.showing_cached_page = true;
                        self.apply_retro_page(page)
                    }
                    _ => Task::none(),
                };
                let backend = self.backend.clone();
                Task::batch([covers, Task::perform(
                    async move { backend.load_retro_page(payload).await },
                    move |result| Message::RetroPageLoaded { generation, offset, result },
                )])
            }
            Message::RetroPageLoaded {
                generation,
                offset,
                result,
            } => {
                if generation != self.retro.generation {
                    return Task::none();
                }
                match result {
                    Ok(mut page) => {
                        page.offset = offset;
                        self.apply_retro_page(page)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] retro: {error}");
                        if self.retro.showing_cached_page {
                            self.retro.status = LoadStatus::Ready;
                            return Task::none();
                        }
                        self.retro.status = LoadStatus::Error(error.clone());
                        self.retro.error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::RetroLibraryArtLoaded { id, art } => {
                self.retro_library_art.insert(id.clone(), art.clone());
                let Some(game) = self.library.game_mut(&id) else { return Task::none() };
                if game.cover_url.is_some() {
                    return Task::none();
                }
                if let Some((url, fallback)) = art {
                    game.cover_url = Some(url.clone());
                    self.retro_cover_tasks([(url, fallback)])
                } else {
                    Task::none()
                }
            }
            Message::RetroPrevPage => {
                if self.retro.offset == 0 || self.retro.status == LoadStatus::Loading {
                    return Task::none();
                }
                let offset = self.retro.offset.saturating_sub(RETRO_PAGE_SIZE);
                self.load_retro_page(offset)
            }
            Message::RetroNextPage => {
                if !self.retro.has_more || self.retro.status == LoadStatus::Loading {
                    return Task::none();
                }
                let offset = self.retro.offset + RETRO_PAGE_SIZE;
                self.load_retro_page(offset)
            }
            Message::RetroOpenDetail(game) => {
                let urls = game.image_urls();
                let detail_task = self.retro_game_task(&game);
                self.store.detail = None;
                self.retro.detail = Some(RetroDetailState {
                    game,
                    detail: None,
                    status: LoadStatus::Loading,
                    error: None,
                    selected_offer: None,
                    offer_detail: None,
                    offer_status: LoadStatus::Idle,
                    offer_error: None,
                });
                Task::batch([self.cover_tasks(urls), detail_task])
            }
            Message::RetroCloseDetail => {
                self.retro.detail = None;
                Task::none()
            }
            Message::RetroDetailLoaded { id, result } => {
                let Some(detail) = self.retro.detail.as_mut() else {
                    return Task::none();
                };
                if detail.game.id != id {
                    return Task::none();
                }
                let urls = match result {
                    Ok(full) => {
                        let urls = full.image_urls();
                        detail.game = full.game.clone();
                        detail.detail = Some(full);
                        detail.status = LoadStatus::Ready;
                        detail.error = None;
                        urls
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] retro (ficha): {error}");
                        detail.status = LoadStatus::Error(error.clone());
                        detail.error = Some(error);
                        Vec::new()
                    }
                };
                self.cover_tasks(urls)
            }
            Message::RetroOfferRequested(offer_id) => {
                let Some(detail) = self.retro.detail.as_mut() else {
                    return Task::none();
                };
                if detail.offer_status == LoadStatus::Loading
                    && detail.selected_offer.as_deref() == Some(offer_id.as_str())
                {
                    return Task::none();
                }
                detail.selected_offer = Some(offer_id.clone());
                detail.offer_detail = None;
                detail.offer_status = LoadStatus::Loading;
                detail.offer_error = None;
                let backend = self.backend.clone();
                let request_id = offer_id.clone();
                Task::perform(
                    async move { backend.load_retro_offer(request_id).await },
                    move |result| Message::RetroOfferLoaded { offer_id, result },
                )
            }
            Message::RetroOfferLoaded { offer_id, result } => {
                let Some(detail) = self.retro.detail.as_mut() else {
                    return Task::none();
                };
                if detail.selected_offer.as_deref() != Some(offer_id.as_str()) {
                    return Task::none();
                }
                match result {
                    Ok(offer) => {
                        detail.offer_detail = Some(offer);
                        detail.offer_status = LoadStatus::Ready;
                        detail.offer_error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] retro (oferta): {error}");
                        detail.offer_detail = None;
                        detail.offer_status = LoadStatus::Error(error.clone());
                        detail.offer_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::SidebarSearchChanged(search) => {
                self.sidebar_search = search;
                Task::none()
            }
            Message::ToggleSidebarGames => {
                self.sidebar_games_open = !self.sidebar_games_open;
                Task::none()
            }
            Message::ToggleProfileMenu => {
                self.profile_menu_open = !self.profile_menu_open;
                Task::none()
            }
            Message::OpenSettingsFromProfile => {
                // Espelha o `onConfigSub("gerais"); onView("config")` do web:
                // menu fecha, aba reseta para Gerais e a tela muda de vez.
                self.profile_menu_open = false;
                self.settings.tab = SettingsTab::General;
                self.update(Message::Navigate(Section::Settings))
            }
            Message::WindowReady(id) => {
                self.window = id;
                match id {
                    Some(id) if matches!(self.screen, Screen::BigPicture) => {
                        window::set_mode(id, window::Mode::Fullscreen)
                    }
                    _ => Task::none(),
                }
            }
            Message::WindowDrag => self.window_task(window::drag),
            Message::WindowMinimize => self.window_task(|id| window::minimize(id, true)),
            Message::WindowToggleMaximize => self.window_task(window::toggle_maximize),
            Message::WindowClose => self.window_task(window::close),
            Message::WindowEvent(event) => self.window_event(event),
            Message::WindowResized(size) => {
                self.game_menu = None;
                self.window_size = size;
                if matches!(self.screen, Screen::BigPicture) {
                    self.console_rail_scroll()
                } else {
                    Task::none()
                }
            }
            Message::EnterBigPicture => self.enter_big_picture(),
            Message::ExitBigPicture => self.exit_big_picture(),
            Message::ConsoleTick => self.console_tick(),
            Message::ConsoleKey(event) => self.console_key(event),
            Message::ConsoleSelectGame(index) => self.console_select_game(index),
            Message::ConsoleSelectTab(tab) => self.console_select_tab(tab),
            Message::SquareArtLoaded(Ok(artes)) => {
                let total = artes.len();
                self.library.square_art.extend(artes);
                eprintln!(
                    "[arcadia-native] arte quadrada: {total} jogo(s) resolvido(s)"
                );
                // As URLs acabaram de entrar: pede os bytes ao cache de capas,
                // senão o card não tem o que desenhar.
                let urls: Vec<String> = self
                    .library
                    .square_art
                    .values()
                    .filter(|url| self.covers.size(Some(url)).is_none())
                    .cloned()
                    .collect();
                self.cover_tasks(urls)
            }
            Message::SquareArtLoaded(Err(error)) => {
                // Sem arte quadrada o card continua na capa vertical — é o
                // fallback desenhado, não uma tela vazia. O log fica para
                // saber se a falha foi de rede ou de chave ausente.
                eprintln!("[arcadia-native] arte quadrada falhou: {error}");
                Task::none()
            }
            Message::ConsoleFocusRail => {
                // Clique num chip de filtro devolve o foco ao trilho sem sair
                // do modo console; o índice atual é preservado.
                self.big_picture.focus_rail(self.big_picture.active_index());
                Task::none()
            }
            Message::ConsolePlay => self.console_action(ACTION_PLAY),
            Message::ConsoleTrailer => self.console_action(ACTION_TRAILER),
            Message::ConsoleFavorite => self.console_action(ACTION_FAVORITE),
            Message::ConsoleMore => self.console_action(ACTION_MORE),
            Message::ConsoleOverviewTab(tab) => {
                self.big_picture.select_overview_tab(tab);
                Task::none()
            }
            Message::ConsoleOverviewMedia(index) => self.console_open_overview_media(index),
            Message::ConsoleOverviewAdvance => {
                self.big_picture.focus_next_overview_media();
                views::big_picture_detail::gallery_scroll_task(self.big_picture.overview_media(), self.window_size.width)
            }
            Message::ConsoleOverviewClosePreview => {
                self.big_picture.close_overview_preview();
                let _ = self.media.pause();
                Task::none()
            }
            Message::ConsoleCloseOverview => {
                self.big_picture.close_overview();
                Task::batch([self.console_sync_media(), self.ensure_console_news()])
            }
            Message::ConsoleMenuAction(index) => {
                let games = console_shelf_for(&self.library, self.big_picture.tab());
                let effect = self.big_picture.press_menu(index, &games);
                self.console_effect(effect)
            }
            Message::ConsoleCloseMenu => {
                self.big_picture.close_menu();
                Task::none()
            }
            Message::ConsoleStoreFocus => {
                if self.big_picture.tab() != ConsoleTab::Stores {
                    return Task::none();
                }
                self.big_picture.begin_store_editing();
                self.console_store_focus_task(true)
            }
            Message::ConsoleStoreOpenDetail(game) => {
                let index = self.console_store_games().iter().position(|item| item.appid == game.appid);
                if let Some(index) = index { self.big_picture.focus_store_game(index); }
                if !self.big_picture.open_store_detail() {
                    return Task::none();
                }
                self.open_store_detail(game)
            }
            Message::ConsoleStoreCategory(index) => {
                if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab() != ConsoleTab::Stores || index >= 9 { return Task::none(); }
                self.console_storefront.category = index;
                self.console_storefront.catalog = false;
                self.store.search = None;
                self.store.query.clear();
                self.big_picture.focus_store_category(index);
                self.sync_console_store();
                let prefetch = self.prefetch_store_front();
                let genres = self.prefetch_store_genres();
                let home = if self.home.games.is_empty() && matches!(self.home.status, LoadStatus::Idle) { self.load_section(Section::Home, false) } else { Task::none() };
                Task::batch([prefetch, genres, home, views::big_picture_store::scroll_task(0, self.window_size.width, true)])
            }
            Message::ConsoleStoreHeroStep(delta) => {
                if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab() != ConsoleTab::Stores { return Task::none(); }
                let count = views::big_picture_store::hero_games(self).len();
                self.console_storefront.step_hero(delta, count);
                self.big_picture.focus_store_hero();
                let game = views::big_picture_store::hero_games(self).get(self.console_storefront.hero).copied().cloned();
                match game { Some(game) => self.prefetch_store(game), None => Task::none() }
            }
            Message::ConsoleStoreHeroOpen => {
                if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab() != ConsoleTab::Stores { return Task::none(); }
                let game = views::big_picture_store::hero_games(self).get(self.console_storefront.hero).copied().cloned();
                match game { Some(game) => self.update(Message::ConsoleStoreOpenDetail(game)), None => Task::none() }
            }
            Message::ConsoleStoreCatalog => {
                if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab() != ConsoleTab::Stores { return Task::none(); }
                self.console_storefront.catalog = true;
                self.store.search = None;
                self.store.query.clear();
                self.sync_console_store();
                self.big_picture.focus_store_game(0);
                views::big_picture_store::scroll_task(0, self.window_size.width, false)
            }
            Message::ConsoleStoreFavorite(id) => self.toggle_console_favorite(id),
            Message::ConsoleStorefrontRetry => {
                if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab()!=ConsoleTab::Stores { return Task::none(); }
                self.console_storefront.status=LoadStatus::Idle;
                self.ensure_console_store()
            }
            Message::ConsoleStorefrontLoaded { language, result } => {
                if self.console_storefront.language != Some(language) { return Task::none(); }
                match result {
                    Ok(feed) => {
                        let urls = feed.specials.iter().chain(&feed.releases).chain(&feed.popular)
                            .flat_map(StoreGame::image_urls).collect();
                        eprintln!("[arcadia-native] vitrine pronta: {} ofertas, {} lançamentos, {} mais vendidos",feed.specials.len(),feed.releases.len(),feed.popular.len());
                        self.console_storefront.feed = feed;
                        self.console_storefront.hero = 0;
                        self.console_storefront.status = LoadStatus::Ready;
                        self.console_storefront.loaded_at = Some(Instant::now());
                        self.sync_console_store();
                        let covers = self.cover_tasks(urls);
                        let ready = self.ensure_console_store();
                        let featured = views::big_picture_store::hero_games(self).first().copied().cloned();
                        let prefetch = featured.map(|game| self.prefetch_store(game)).unwrap_or_else(Task::none);
                        Task::batch([covers, ready, prefetch])
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] vitrine: {error}");
                        self.console_storefront.status = LoadStatus::Error(error);
                        Task::none()
                    }
                }
            }
            Message::ConsoleStoreCloseDetail => {
                self.big_picture.close_store_detail();
                self.store.detail = None;
                self.animated_cards.clear_cards();
                self.card_videos.clear();
                Task::none()
            }
            Message::ConsoleMediaLoaded { id, media } => {
                self.cache_game_media(&id, media.clone());
                if !matches!(self.screen, Screen::BigPicture)
                    || !self.big_picture.overview_open()
                    || self.console_media_id.as_deref() != Some(id.as_str())
                {
                    return Task::none();
                }
                eprintln!(
                    "[arcadia-native] trailer (console): {} movie(s){}",
                    media.movies.len(),
                    media
                        .notice
                        .as_deref()
                        .map(|notice| format!(" — {notice}"))
                        .unwrap_or_default()
                );
                let urls: Vec<String> = media.overview.background.iter().cloned().chain(media.movies.iter().filter_map(|movie| movie.thumb.clone()))
                    .chain(media.screenshots.iter().filter_map(|shot| shot.thumb_or_full().map(str::to_string)))
                    .collect();
                self.big_picture.sync_overview_media(usize::from(!media.movies.is_empty()) + media.screenshots.len());
                self.game_media = Some(media);
                let covers = self.cover_tasks(urls);
                let sources = self.game_media.as_ref().and_then(|data| data.movies.first())
                    .map(|movie| media::TrailerSources { hls: movie.hls.clone(), mp4: movie.mp4.clone(), webm: movie.webm.clone() });
                let playback = match sources {
                    Some(sources) => match self.media.open(&sources) {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::MediaError(error)),
                    },
                    None => Task::none(),
                };
                Task::batch([covers, playback])
            }
            Message::ConsoleFavoriteDone { id, result } => {
                self.favorite_busy = None;
                match result {
                    Ok(games) => {
                        self.library.ready(games);
                        self.big_picture
                            .sync_games(self.library.visible_all().len());
                        self.focus_console_game(&id);
                        let (title, favorite) = self
                            .library
                            .game(&id)
                            .map(|game| (game.title.clone(), game.favorite))
                            .unwrap_or_else(|| (id.clone(), false));
                        self.big_picture.set_notice(if favorite {
                            self.i18n.t_args(
                                "native.console.notice.favorite_added",
                                &[("title", &title)],
                            )
                        } else {
                            self.i18n.t_args(
                                "native.console.notice.favorite_removed",
                                &[("title", &title)],
                            )
                        });
                        let urls = self.library.image_urls();
                        self.cover_tasks(urls)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] favorito: {error}");
                        let title = self
                            .library
                            .game(&id)
                            .map(|game| game.title.clone())
                            .unwrap_or(id);
                        self.big_picture.set_notice(self.i18n.t_args(
                            "native.console.notice.favorite_failed",
                            &[("title", &title), ("error", &error)],
                        ));
                        Task::none()
                    }
                }
            }
            Message::ConsoleNewsLoaded(result) => {
                match result {
                    Ok(items) => {
                        eprintln!("[arcadia-native] notícias: {} item(ns)", items.len());
                        self.big_picture.sync_news(items.len());
                        self.console_news.items = items;
                        self.console_news.status = LoadStatus::Ready;
                        self.console_news.error = None;
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] notícias: {error}");
                        self.console_news.status = LoadStatus::Error(error.clone());
                        self.console_news.error = Some(error);
                    }
                }
                Task::none()
            }
            Message::ConsoleNewsSelected(index) => {
                self.big_picture.select_news(index);
                Task::none()
            }
            Message::ConsoleNewsReload => self.load_console_news(),

            Message::MediaLoaded {
                game_id,
                media: game_media,
            } => {
                self.cache_game_media(&game_id, game_media.clone());
                // Resposta do trailer do desktop não pode abrir mídia por cima
                // do painel do console; o console usa `ConsoleMediaLoaded`.
                if matches!(self.screen, Screen::BigPicture) {
                    return Task::none();
                }
                // Clique rápido em outro jogo: a resposta antiga não pode
                // trocar screenshots/descrição da ficha atual.
                if self.current_game().map(|game| game.id.as_str()) != Some(game_id.as_str()) {
                    return Task::none();
                }
                eprintln!(
                    "[arcadia-native] trailer: {} movie(s), {} screenshot(s){}",
                    game_media.movies.len(),
                    game_media.screenshots.len(),
                    game_media
                        .notice
                        .as_deref()
                        .map(|notice| format!(" — {notice}"))
                        .unwrap_or_default()
                );
                // Ficha nova: os cards animados da anterior não servem mais.
                // A mídia do perfil continua — fechar a ficha do jogo não
                // fecha o perfil.
                self.animated_cards.clear_cards();
                self.card_videos.clear();
                self.game_media = Some(game_media);
                self.selected_screenshot = 0;
                self.gallery_trailer = true;

                let mut urls = Vec::new();
                if let Some(media) = self.game_media.as_ref() {
                    for movie in &media.movies {
                        if let Some(url) = movie.thumb.as_deref() {
                            urls.push(url.to_string());
                        }
                    }
                    for screenshot in &media.screenshots {
                        if let Some(url) = screenshot.thumb_or_full() {
                            urls.push(url.to_string());
                        }
                    }
                    if let Some(full) = media
                        .screenshots
                        .first()
                        .and_then(|screenshot| screenshot.full_or_thumb())
                    {
                        urls.push(full.to_string());
                    }
                }
                let about = self
                    .game_media
                    .as_ref()
                    .and_then(|media| media.about.clone());
                if let Some(raw) = about.as_deref() {
                    urls.extend(crate::rich_text::description_images(raw));
                }
                self.register_description_videos(about.as_deref());
                let images = self.cover_tasks(urls);

                // Um trailer local escolhido pelo usuário tem prioridade sobre
                // um `game:sysinfo` que ainda estava em voo.
                if self.trailer_override {
                    return images;
                }
                // Prioridade hls → mp4 → webm fica no wrapper de mídia.
                let sources = self
                    .game_media
                    .as_ref()
                    .and_then(|game_media| game_media.movies.first())
                    .map(|movie| media::TrailerSources {
                        hls: movie.hls.clone(),
                        mp4: movie.mp4.clone(),
                        webm: movie.webm.clone(),
                    });
                let playback = match sources {
                    Some(sources) => match self.media.open(&sources) {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::MediaError(error)),
                    },
                    None => Task::none(),
                };
                Task::batch([images, playback, views::game_detail::scroll_to_top()])
            }
            Message::AchievementsLoaded { game_id, result } => {
                let mut icons = Vec::new();
                if let Some(panels) = self.game_panels.get_mut(&game_id) {
                    match result {
                        Ok(data) => {
                            eprintln!(
                                "[arcadia-native] conquistas: {} item(ns) para {game_id}",
                                data.items.len()
                            );
                            panels.achievements.items = data.items;
                            panels.achievements.note = data.note;
                            panels.achievements.status = LoadStatus::Ready;
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] conquistas: {error}");
                            panels.achievements.items.clear();
                            panels.achievements.note = None;
                            panels.achievements.status = LoadStatus::Error(error);
                        }
                    }
                    icons = panels
                        .achievements
                        .items
                        .iter()
                        .filter_map(|item| item.icon.clone())
                        .collect();
                }
                self.cover_tasks(icons)
            }
            Message::ProtonDbLoaded { game_id, result } => {
                if let Some(panels) = self.game_panels.get_mut(&game_id) {
                    match result {
                        Ok(info) => {
                            panels.proton.info = info;
                            panels.proton.note = None;
                            panels.proton.status = LoadStatus::Ready;
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] protondb: {error}");
                            panels.proton.info = None;
                            panels.proton.status = LoadStatus::Error(error);
                        }
                    }
                }
                Task::none()
            }
            Message::GameStatsLoaded { game_id, result } => {
                if let Some(panels) = self.game_panels.get_mut(&game_id) {
                    match result {
                        Ok(stats) => {
                            panels.stats.stats = stats;
                            panels.stats.note = None;
                            panels.stats.status = LoadStatus::Ready;
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] stats: {error}");
                            panels.stats.stats = None;
                            panels.stats.status = LoadStatus::Error(error);
                        }
                    }
                }
                Task::none()
            }
            Message::SelectScreenshot(index) => {
                self.selected_screenshot = index;
                self.gallery_trailer = false;
                let _ = self.media.pause();
                let url = self
                    .game_media
                    .as_ref()
                    .and_then(|media| media.screenshots.get(index))
                    .and_then(|screenshot| screenshot.full_or_thumb())
                    .map(str::to_string);
                match url {
                    Some(url) => self.cover_tasks(vec![url]),
                    None => Task::none(),
                }
            }
            Message::SelectTrailer => {
                self.gallery_trailer = true;
                if self.media.is_active() && self.media.play().is_ok() {
                    return Task::none();
                }
                let sources = self
                    .game_media
                    .as_ref()
                    .and_then(|media| media.movies.first())
                    .map(|movie| media::TrailerSources {
                        hls: movie.hls.clone(),
                        mp4: movie.mp4.clone(),
                        webm: movie.webm.clone(),
                    });
                match sources {
                    Some(sources) => match self.media.open(&sources) {
                        Ok(()) => Task::none(),
                        Err(error) => Task::done(Message::MediaError(error)),
                    },
                    None => Task::none(),
                }
            }
            Message::RequirementsTabSelected(tab) => {
                self.requirements_tab = tab;
                Task::none()
            }
            Message::ReloadGameData => {
                let Some(game) = self.current_game().cloned() else {
                    return Task::none();
                };
                self.game_panels.remove(&game.id);
                self.media.clear();
                self.game_media = None;
                self.selected_screenshot = 0;
                self.gallery_trailer = false;
                self.requirements_tab = RequirementsTab::default();
                self.reset_trailer_state();
                let media = self.game_media_task(&game);
                let panels = self.game_panels_task(&game);
                Task::batch([media, panels])
            }
            Message::MediaTick => {
                self.media.tick();
                Task::none()
            }
            Message::MediaTogglePlayback => match self.media.toggle() {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            },
            Message::MediaSeek(seconds) => match self.media.seek(seconds) {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            },
            Message::MediaError(error) => {
                eprintln!("[arcadia-native] trailer: {error}");
                self.media.report_error(error);
                Task::none()
            }
            Message::OpenTrailerPicker => self.open_trailer_picker(),
            Message::CloseTrailerPicker => {
                self.trailer_picker.open = false;
                Task::none()
            }
            Message::TrailerQueryChanged(query) => {
                self.trailer_picker.query = query;
                Task::none()
            }
            Message::TrailerSearchSubmitted => self.submit_trailer_search(),
            Message::TrailerSearchDone { query, result } => {
                if !self.trailer_picker.open
                    || self.trailer_picker.downloading.is_some()
                    || self.trailer_picker.query.trim() != query
                {
                    return Task::none();
                }

                match result {
                    Ok(results) => {
                        let thumbnails: Vec<String> = results
                            .iter()
                            .filter_map(|result| result.thumbnail.clone())
                            .collect();
                        eprintln!(
                            "[arcadia-native] trailer: {} vídeo(s) para {query:?}",
                            results.len()
                        );
                        self.trailer_picker.results = results;
                        self.trailer_picker.status = LoadStatus::Ready;
                        self.cover_tasks(thumbnails)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] trailer: {error}");
                        self.trailer_picker.results.clear();
                        self.trailer_picker.status = LoadStatus::Error(error);
                        Task::none()
                    }
                }
            }
            Message::TrailerPathLoaded { game_id, result } => {
                if !self.trailer_picker.open || self.trailer_picker.game_id != game_id {
                    return Task::none();
                }
                match result {
                    Ok(path) => self.trailer_picker.existing_path = path,
                    Err(error) => eprintln!("[arcadia-native] trailer local: {error}"),
                }
                Task::none()
            }
            Message::TrailerDownloadRequested(video) => {
                if !self.trailer_picker.open || self.trailer_picker.downloading.is_some() {
                    return Task::none();
                }
                let id = self.trailer_picker.game_id.clone();
                eprintln!(
                    "[arcadia-native] trailer: baixando {:?} ({})",
                    video.title, video.url
                );
                self.trailer_picker.downloading = Some(video.clone());
                self.trailer_picker.download_error = None;
                let backend = self.backend.clone();
                let message_game_id = id.clone();
                let message_url = video.url.clone();
                Task::perform(
                    async move { backend.download_trailer(id, video.url).await },
                    move |result| Message::TrailerDownloadDone {
                        game_id: message_game_id,
                        url: message_url,
                        result,
                    },
                )
            }
            Message::TrailerDownloadDone {
                game_id,
                url,
                result,
            } => {
                let still_current = self.trailer_picker.open
                    && self.trailer_picker.game_id == game_id
                    && self
                        .trailer_picker
                        .downloading
                        .as_ref()
                        .is_some_and(|video| video.url == url);
                if !still_current {
                    // O usuário fechou/trocou de jogo durante o download: o
                    // arquivo fica no disco para "Abrir trailer baixado".
                    eprintln!(
                        "[arcadia-native] trailer: download de {url} terminou fora do seletor"
                    );
                    return Task::none();
                }

                self.trailer_picker.downloading = None;
                match result {
                    Ok(path) => {
                        eprintln!("[arcadia-native] trailer baixado: {path}");
                        self.trailer_picker.open = false;
                        self.open_local_trailer(path)
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] trailer: {error}");
                        self.trailer_picker.download_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::TrailerOpenDownloaded => {
                let Some(path) = self.trailer_picker.existing_path.clone() else {
                    return Task::none();
                };
                self.trailer_picker.open = false;
                self.open_local_trailer(path)
            }
            Message::LaunchGame(id) => self.request_launch(id),
            Message::StopGame => self.request_stop(),
            Message::LaunchGameDone { id, result } => self.finish_launch(id, result),
            Message::StopGameDone(result) => self.finish_stop(result),
            Message::LaunchEventsTick => {
                // O cache de capas é limitado; uma capa despejada que continua
                // na tela volta à fila e é recarregada do disco no mesmo tick.
                let reload = self.covers.take_misses();
                let events = self.drain_launch_events();
                let library = if self.library_dirty {
                    self.light_library_refresh()
                } else {
                    Task::none()
                };
                Task::batch([events, library, self.cover_tasks(reload)])
            }
            Message::OpenLaunchOptions(id) => self.open_launch_options(id),
            Message::CloseLaunchOptions => {
                self.launch_options.open = false;
                Task::none()
            }
            Message::LaunchOptionsLoaded { id, result } => {
                if !self.launch_options.open || self.launch_options.game_id != id {
                    return Task::none();
                }
                match result {
                    Ok(envelope) => {
                        // `envelope.default_prefix` é o default das ferramentas
                        // Wine da UI web; o launch nativo tem fallback próprio
                        // (já gravado no estado ao abrir o diálogo).
                        if self.launch_options.mode.as_deref() == Some("exe")
                            && envelope.settings.exe_path.trim().is_empty()
                        {
                            self.launch_options.mode = None;
                        }
                        self.launch_options.settings = Some(envelope.settings);
                        if let Some(settings) = self.launch_options.settings.as_ref() {
                            self.launch_options.emulator_args_text =
                                settings.emulator_args.join(" ");
                        }
                        self.launch_options.status = LoadStatus::Ready;
                        self.launch_options.saved = false;
                        self.launch_options.dirty = false;
                        self.launch_options.error = None;
                    }
                    Err(error) => {
                        self.launch_options.status = LoadStatus::Error(error);
                    }
                }
                Task::none()
            }
            Message::LaunchOptionsEmulatorsLoaded { id, result } => {
                if !self.launch_options.open || self.launch_options.game_id != id {
                    return Task::none();
                }
                match result {
                    Ok(options) => {
                        self.launch_options.emulators = options.emulators;
                        self.launch_options.emulator_statuses = options.statuses;
                        self.launch_options.emulators_error = None;
                    }
                    Err(error) => {
                        self.launch_options.emulators = Vec::new();
                        self.launch_options.emulator_statuses = Vec::new();
                        self.launch_options.emulators_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::LaunchOptionsEmulatorChanged(value) => {
                let emulator_id = value.trim().to_string();
                let requires_core = self.launch_options.emulators.iter().any(|item| {
                    item.id == emulator_id && (item.requires_core || item.id == "retroarch")
                });
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.emulator_id = (!emulator_id.is_empty()).then_some(emulator_id);
                    if !requires_core {
                        settings.emulator_core_path.clear();
                    }
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsRomChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.rom_path = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsCoreChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.emulator_core_path = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsEmulatorArgsChanged(value) => {
                if self.launch_options.settings.is_some() {
                    self.launch_options.emulator_args_text = value.clone();
                    if let Some(settings) = self.launch_options.settings.as_mut() {
                        settings.emulator_args = crate::backend::parse_emulator_args(&value);
                    }
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsValidated { id, patch, result } => {
                if self.launch_options.game_id != id || !self.launch_options.saving {
                    return Task::none();
                }
                match result {
                    Ok(()) => self.save_launch_options_patch(id, patch),
                    Err(error) => {
                        self.launch_options.saving = false;
                        self.launch_options.dirty = true;
                        self.launch_options.error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::LaunchOptionsPrefixChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.prefix_path = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsExeChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    if value.trim().is_empty() && self.launch_options.mode.as_deref() == Some("exe")
                    {
                        self.launch_options.mode = None;
                    }
                    settings.exe_path = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsArgsChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.game_args = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsHudChanged(value) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.dxvk_hud = value;
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsToggle { key, value } => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    settings.set_toggle(key, value);
                    self.launch_options.mark_dirty();
                }
                Task::none()
            }
            Message::LaunchOptionsEnvNameChanged(value) => {
                self.launch_options.env_name = value;
                Task::none()
            }
            Message::LaunchOptionsEnvValueChanged(value) => {
                self.launch_options.env_value = value;
                Task::none()
            }
            Message::LaunchOptionsEnvAdd => {
                self.add_launch_env_var();
                Task::none()
            }
            Message::LaunchOptionsEnvRemove(index) => {
                if let Some(settings) = self.launch_options.settings.as_mut() {
                    if index < settings.env_vars.len() {
                        settings.env_vars.remove(index);
                        self.launch_options.mark_dirty();
                    }
                }
                Task::none()
            }
            Message::LaunchOptionsAdvancedToggled => {
                self.launch_options.toggle_advanced();
                Task::none()
            }
            Message::LaunchOptionsModeChanged(mode) => {
                if let Some(value) = mode.as_deref() {
                    if !matches!(value, "steam" | "exe") {
                        return Task::none();
                    }
                    if value == "exe"
                        && self
                            .launch_options
                            .settings
                            .as_ref()
                            .map_or(true, |settings| settings.exe_path.trim().is_empty())
                    {
                        self.launch_options.error =
                            Some(self.i18n.t("native.game.exe_required_mode").to_string());
                        return Task::none();
                    }
                }
                self.launch_options.mode = mode;
                self.launch_options.error = None;
                Task::none()
            }
            Message::LaunchOptionsSave => self.save_launch_options(),
            Message::LaunchOptionsSaved { id, result } => {
                if self.launch_options.game_id != id {
                    return Task::none();
                }
                self.launch_options.saving = false;
                match result {
                    Ok(settings) => {
                        // Edições feitas enquanto o save estava em voo não
                        // podem ser sobrescritas pela resposta do servidor.
                        if !self.launch_options.dirty {
                            self.launch_options.emulator_args_text =
                                settings.emulator_args.join(" ");
                            self.launch_options.settings = Some(settings);
                            self.launch_options.saved = true;
                        }
                        self.launch_options.status = LoadStatus::Ready;
                        self.launch_options.error = None;
                    }
                    Err(error) => {
                        self.launch_options.saved = false;
                        self.launch_options.dirty = true;
                        self.launch_options.error = Some(error);
                    }
                }
                Task::none()
            }
            Message::LaunchConfigLoaded(result) => {
                let mut check_task = Task::none();
                match result {
                    Ok(config) => {
                        self.i18n
                            .set_lang(Lang::from_code(&config.language).unwrap_or_default());
                        self.launch.minimize_on_launch = config.minimize_on_game_launch;
                        self.big_picture.set_motion(!config.no_anim);
                        self.launch_reduce_motion = config.no_anim;
                        self.notifications_enabled = config.notifications_enabled;
                        self.hide_changelog_on_start = config.hide_changelog_on_start;
                        if !config.notifications_enabled {
                            self.toasts.clear();
                        }
                        if config.check_updates_on_start {
                            check_task = self.start_update_check(false);
                        }
                    }
                    Err(error) => {
                        // Preferência opcional: sem config, não minimiza e o
                        // resto do app segue normalmente (pt-BR de fallback).
                        eprintln!("[arcadia-native] minimize_on_game_launch indisponível: {error}");
                    }
                }
                check_task
            }
            Message::BootVideoReady(result) => {
                if !self.boot.active {
                    return Task::none();
                }
                match result {
                    Ok(Some(path)) => {
                        let local = path.to_string_lossy().into_owned();
                        self.boot.path = Some(local.clone());
                        if let Err(error) = self.boot.media.open_local(local) {
                            eprintln!("[arcadia-native] vídeo de boot indisponível: {error}");
                            self.dismiss_splash();
                        }
                    }
                    Ok(None) => self.dismiss_splash(),
                    Err(error) => {
                        eprintln!("[arcadia-native] boot:video falhou: {error}");
                        self.dismiss_splash();
                    }
                }
                Task::none()
            }
            Message::BootVideoTick => {
                if self.boot.active {
                    self.boot_tick();
                }
                Task::none()
            }
            Message::BootVideoKey(event) => {
                if self.boot.active && matches!(event, keyboard::Event::KeyPressed { .. }) {
                    self.dismiss_splash();
                }
                Task::none()
            }
            Message::DismissSplash => {
                self.dismiss_splash();
                Task::none()
            }
            Message::UpdateSnapshotLoaded(result) => {
                match result {
                    Ok(snapshot) => {
                        if let Some(error) = snapshot.error.clone() {
                            eprintln!("[arcadia-native] update snapshot parcial: {error}");
                        }
                        self.update.snapshot = Some(snapshot);
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] update snapshot falhou: {error}");
                        self.update.last_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::AccountStatusLoaded(result) => {
                // Respostas/edições em voo pertencem à conta anterior.
                self.game_menu = None;
                self.game_edit = None;
                self.game_metadata = None;
                self.library_overrides = LibraryOverrideState::default();
                match result {
                Ok(Some(session)) => {
                    self.account.apply_session(session);
                    let community = self.sync_community_viewer();
                    let profile = self.account_profile_task();
                    if matches!(self.screen, Screen::Section(Section::Friends))
                        && matches!(self.friends.status, LoadStatus::Idle)
                    {
                        Task::batch([
                            profile,
                            self.load_section(Section::Friends, false),
                            community,
                        ])
                    } else {
                        Task::batch([profile, community])
                    }
                }
                Ok(None) => {
                    self.account.signed_out();
                    let community = self.sync_community_viewer();
                    let mut tasks = vec![community];
                    // Deslogado: o avatar vem do perfil local (web: `profile.avatar`).
                    tasks.push(self.sync_fallback_avatar());
                    if !self.account.auto_auth_shown {
                        self.account.auto_auth_shown = true;
                        self.account.open_auth(false);
                        tasks.push(self.auth_focus_task());
                    }
                    Task::batch(tasks)
                }
                Err(error) => {
                    eprintln!("[arcadia-native] conta: {error}");
                    self.account.status = AccountStatus::Error;
                    self.account.error = Some(account_error_text(&self.i18n, &error));
                    let mut tasks = Vec::new();
                    tasks.push(self.sync_fallback_avatar());
                    if !self.account.auto_auth_shown {
                        self.account.auto_auth_shown = true;
                        self.account.open_auth(false);
                        self.account.auth.error = Some(account_error_text(&self.i18n, &error));
                        tasks.push(self.auth_focus_task());
                    }
                    Task::batch(tasks)
                }
                }
            },
            Message::AccountProfileLoaded(result) => {
                if !self.account.is_signed_in() {
                    return Task::none();
                }
                match result {
                    Ok(profile) => {
                        let avatar_url = profile.avatar_url.clone();
                        let mut still_urls: Vec<String> = [&profile.banner_url, &profile.background_url]
                            .into_iter()
                            .flatten()
                            .filter(|url| !url.trim().is_empty())
                            .cloned()
                            .collect();
                        still_urls.dedup();
                        self.account.profile_ready(profile);
                        // Banner e fundo têm fetch estático próprio; o do
                        // avatar é o `avatar:load` mais abaixo.
                        let banner_task = if still_urls.is_empty() {
                            Task::none()
                        } else {
                            self.cover_tasks(still_urls)
                        };
                        let avatar_task = match avatar_url {
                            Some(url)
                                if self.account.avatar_url.as_deref() != Some(url.as_str()) =>
                            {
                                self.account.avatar_url = Some(url.clone());
                                self.account.avatar = None;
                                self.avatar_load_task(url)
                            }
                            Some(_) => Task::none(),
                            None => {
                                // Perfil sem avatar utilizável (campo vazio ou
                                // esquema que o `avatar:load` não abre, como um
                                // `data:` antigo gravado no banco). Sem este log
                                // o "letra no lugar da foto" era invisível.
                                if self.account.profile.as_ref().is_some_and(|profile| {
                                    profile
                                        .avatar_url
                                        .as_deref()
                                        .is_some_and(|url| !url.trim().is_empty())
                                }) {
                                    eprintln!(
                                        "[arcadia-native] avatar_url presente mas não utilizável pelo avatar:load"
                                    );
                                }
                                self.account.avatar_url = None;
                                self.account.avatar = None;
                                // Mesmo contrato do web: sem avatar da conta, o
                                // `profile.avatar` local assume (Sidebar.tsx:101).
                                self.fallback_avatar_task()
                            }
                        };
                        // A URL da conta precisa estar definida antes de agendar
                        // a animação; não espere o download estático terminar.
                        let media_task = self.ensure_profile_media();
                        Task::batch([banner_task, avatar_task, media_task])
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] perfil da conta: {error}");
                        self.account
                            .profile_failed(account_error_text(&self.i18n, &error));
                        Task::none()
                    }
                }
            }
            Message::ProfileStatsLoaded(result) => {
                match result {
                    Ok(stats) => self.account.stats = Some(stats),
                    Err(error) => eprintln!("[arcadia-native] profile:stats falhou: {error}"),
                }
                Task::none()
            }
            Message::AnimatedMediaLoaded { url, media } => {
                match media {
                    Some(media) => self.animated_cards.insert_decoded(
                        &url,
                        media,
                        crate::animated_cards::MediaGroup::Profile,
                    ),
                    None => {
                        self.animated_cards.clear_in_flight(&url);
                        eprintln!(
                            "[arcadia-native] mídia animada: download falhou para {url}"
                        );
                    }
                }
                Task::none()
            }
            Message::ReloadAccount => {
                self.account.status = AccountStatus::Loading;
                self.account.error = None;
                self.account_status_task()
            }
            Message::AccountAvatarLoaded { url, handle } => {
                // Aceita tanto o avatar da conta quanto o fallback local: quem
                // atualiza é a comparação da URL.
                //
                // Não zera `fallback_avatar_url` quando a conta pinta a mesma
                // URL: o contrato do Sidebar.tsx é o local continuar de reserva
                // (`contaPerfil?.avatar_url || profile.avatar`). Zerar aqui
                // fazia o próximo `sync_fallback_avatar` não achar nada e a
                // sidebar cair na inicial depois de um reload.
                let is_account = self.account.avatar_url.as_deref() == Some(url.as_str());
                let is_fallback =
                    self.account.fallback_avatar_url.as_deref() == Some(url.as_str());
                if is_account || is_fallback {
                    self.account.avatar = handle;
                    // A sidebar pinta o avatar o tempo todo, não só na tela de
                    // perfil: o GIF precisa entrar no pool assim que a URL é
                    // conhecida, senão fica no primeiro quadro para sempre.
                    return self.ensure_profile_media();
                }
                Task::none()
            }
            Message::OpenAuth { required } => {
                self.account.open_auth(required);
                self.auth_focus_task()
            }
            Message::AuthClosed => {
                self.account.close_auth();
                Task::none()
            }
            Message::AuthModeChanged(mode) => {
                if self.account.auth.mode == mode {
                    return Task::none();
                }
                self.account.auth.mode = mode;
                self.account.auth.error = None;
                self.account.auth.password.clear();
                self.auth_focus_task()
            }
            Message::AuthEmailChanged(email) => {
                self.account.auth.email = email;
                Task::none()
            }
            Message::AuthUsernameChanged(username) => {
                self.account.auth.username = username;
                Task::none()
            }
            Message::AuthPasswordChanged(password) => {
                self.account.auth.password = password;
                Task::none()
            }
            Message::AuthSubmit => self.submit_auth(),
            Message::AuthDone(result) => {
                self.account.auth.sending = false;
                match result {
                    Ok(success) => {
                        self.account.auth.open = false;
                        self.account.auth.required = false;
                        self.account.auth.password.clear();
                        self.account.auth.error = None;
                        self.account.error = None;
                        self.account.notice = success.username_real.map(|username| {
                            self.i18n
                                .t_args("native.account.created", &[("username", &username)])
                        });
                        self.account.status = AccountStatus::Loading;
                        // A conta mudou o escopo da biblioteca e dos amigos.
                        self.friends = FriendsState::default();
                        self.friend_media.clear();
                        self.account_status_task()
                    }
                    Err(error) => {
                        let message = account_error_text(&self.i18n, &error);
                        self.account.auth_failed(message);
                        self.auth_focus_task()
                    }
                }
            }
            Message::AccountKey(event) => self.account_key(event),
            Message::SignOutRequested => {
                if !self.account.is_signed_in() || self.account.status == AccountStatus::Loading {
                    return Task::none();
                }
                self.profile_menu_open = false;
                self.account.status = AccountStatus::Loading;
                self.account.error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.sign_out().await },
                    Message::SignOutDone,
                )
            }
            Message::SignOutDone(result) => {
                let error = result.err();
                if let Some(error) = error.as_ref() {
                    eprintln!("[arcadia-native] sair da conta: {error}");
                }
                self.account.signed_out();
                // Sem sessão o perfil local assume o avatar, como no web.
                let fallback = self.sync_fallback_avatar();
                let community = self.sync_community_viewer();
                self.account.auto_auth_shown = true;
                self.account.open_auth(true);
                self.account.auth.error = error.map(|error| account_error_text(&self.i18n, &error));
                self.friends = FriendsState::default();
                self.friends.status = LoadStatus::Ready;
                self.friend_media.clear();
                self.friends.data = Some(FriendsLoad::NotLoggedIn);
                if matches!(self.screen, Screen::Profile) {
                    self.page_entered = Instant::now();
                    self.page_direction = -1.0;
                    self.screen = Screen::Library;
                }
                Task::batch([self.auth_focus_task(), community, fallback])
            }
            Message::OpenProfile => {
                self.profile_menu_open = false;
                if self.account.is_signed_in() {
                    self.page_entered = Instant::now();
                    self.page_direction = 1.0;
                    self.screen = Screen::Profile;
                    // O card de Estatísticas depende de `profile:stats`; a web
                    // busca no `open` da `ProfilePage`. Dispara junto com a
                    // troca de tela para o card já pintar com números.
                    let mut tasks = vec![self.profile_stats_task()];

                    // Avatar/banner/fundo: decode em worker; o pool só toca
                    // os quadros prontos, sem bloquear a troca de tela.
                    tasks.push(self.ensure_profile_media());

                    // Banner e fundo: a `ProfilePage.tsx` pinta `profile.banner`
                    // e `profile.background`; ambos vêm do servidor e precisam
                    // do fetch estático para ter algo antes do primeiro frame
                    // animado. O avatar já tem o `avatar:load` da sidebar.
                    let mut still_urls = Vec::new();
                    if let Some(profile) = self.account.profile.as_ref() {
                        for url in [profile.banner_url.as_deref(), profile.background_url.as_deref()]
                            .into_iter()
                            .flatten()
                            .filter(|url| !url.trim().is_empty())
                        {
                            still_urls.push(url.to_string());
                        }
                    }
                    if !still_urls.is_empty() {
                        tasks.push(self.cover_tasks(still_urls));
                    }

                    return Task::batch(tasks);
                } else {
                    self.account.open_auth(false);
                    return self.auth_focus_task();
                }
            }
            Message::ProfileEditToggled => {
                self.account.editor.editing = !self.account.editor.editing;
                self.account.editor.error = None;
                self.account.editor.notice = None;
                if self.account.editor.editing {
                    // Abrir direto em "Geral" é o que o web faz: o botão
                    // "Editar perfil" sempre entra pela primeira aba.
                    self.account.editor.tab = ProfileEditTab::General;
                    if let Some(profile) = self.account.profile.as_ref() {
                        let avatar_path = self.account.editor.avatar_path.clone();
                        let pick_busy = self.account.editor.pick_busy.clone();
                        let uploading_avatar = self.account.editor.uploading_avatar;
                        let uploading_background = self.account.editor.uploading_background;
                        let uploading_banner = self.account.editor.uploading_banner;
                        let mut editor = ProfileEditorState::from_profile(profile);
                        editor.editing = true;
                        editor.avatar_path = avatar_path;
                        editor.pick_busy = pick_busy;
                        editor.uploading_avatar = uploading_avatar;
                        editor.uploading_background = uploading_background;
                        editor.uploading_banner = uploading_banner;
                        // O painel de prévia do editor pinta banner e fundo;
                        // sem o fetch aqui a primeira pintura fica vazia.
                        let urls: Vec<String> = [
                            editor.background_url.clone(),
                            editor.banner_url.clone(),
                        ]
                        .into_iter()
                        .filter(|url| !url.trim().is_empty())
                        .collect();
                        self.account.editor = editor;
                        return self.cover_tasks(urls);
                    }
                }
                Task::none()
            }
            Message::ProfileEditTabChanged(tab) => {
                self.account.editor.tab = tab;
                self.account.editor.error = None;
                Task::none()
            }
            Message::ProfileEditKey(event) => {
                let fechar = matches!(
                    event,
                    keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }
                );
                if fechar && self.account.editor.pick_busy.is_none() {
                    self.account.editor.editing = false;
                    self.account.editor.error = None;
                    self.account.editor.notice = None;
                }
                Task::none()
            }
            Message::CheckUpdates(manual) => self.start_update_check(manual),
            Message::UpdateCheckDone { manual, result } => {
                let was_open = self.update.dialog.is_some();
                let task = self.finish_update_check(manual, result);
                if !was_open && self.update.dialog.is_some() && self.window.is_some()
                    && !matches!(self.screen, Screen::BigPicture) && !self.account.auth.open
                {
                    self.update.backdrop_generation = self.update.backdrop_generation.wrapping_add(1);
                    self.update.backdrop_waiting = true;
                }
                task
            }
            Message::UpdateBackdropFrame => {
                if self.update.backdrop_waiting && self.update.backdrop_capture_requested.is_none() {
                    if let Some(id) = self.window {
                        let generation = self.update.backdrop_generation;
                        self.update.backdrop_capture_requested = Some(generation);
                        return window::screenshot(id)
                            .map(move |shot| Message::UpdateBackdropCaptured(generation, shot))
                            .chain(Task::done(Message::UpdateBackdropFailed(generation)));
                    }
                    self.update.backdrop_waiting = false;
                }
                Task::none()
            }
            Message::UpdateBackdropCaptured(generation, shot) => {
                if self.update.backdrop_capture_requested != Some(generation) {
                    return Task::none();
                }
                self.update.backdrop_capture_requested = None;
                if generation != self.update.backdrop_generation || !self.update.backdrop_waiting {
                    return Task::none();
                }
                self.update.backdrop_waiting = false;
                Task::perform(async move {
                    tokio::task::spawn_blocking(move || blur_update_backdrop(shot)).await.ok().flatten()
                }, move |handle| Message::UpdateBackdropBlurred(generation, handle))
            }
            Message::UpdateBackdropFailed(generation) => {
                if self.update.backdrop_capture_requested == Some(generation) {
                    self.update.backdrop_capture_requested = None;
                    if generation == self.update.backdrop_generation {
                        self.update.backdrop_waiting = false;
                    }
                }
                Task::none()
            }
            Message::UpdateBackdropBlurred(generation, handle) => {
                if generation == self.update.backdrop_generation && self.update.dialog.is_some() {
                    self.update.backdrop = handle;
                }
                Task::none()
            }
            Message::UpdateDialogDismiss => self.dismiss_update_dialog(),
            Message::UpdateNoop => Task::none(),
            Message::UpdateDownloadRequested => self.start_update_download(),
            Message::UpdateInstallRequested => self.start_update_install(),
            Message::UpdateRetry => self.retry_update(),
            Message::UpdateActionDone { action, result } => {
                self.finish_update_action(action, result)
            }
            Message::UpdatePollTick => self.poll_update_snapshot(),
            Message::UpdateSnapshotPolled(result) => {
                self.apply_polled_snapshot(result);
                Task::none()
            }
            Message::UpdateAdvisedDone(result) => {
                if let Err(error) = result {
                    eprintln!("[arcadia-native] update:packaged:jaAvisado falhou: {error}");
                }
                Task::none()
            }
            Message::ToastTick => {
                self.toasts.advance(50);
                Task::none()
            }
            Message::DismissToast(id) => {
                self.toasts.dismiss(id);
                Task::none()
            }
            Message::AchievementsPoll => {
                let backend = self.backend.clone();
                Task::perform(async move { backend.achievements_tick().await }, |_| {
                    Message::AchievementsPollDone
                })
            }
            Message::AchievementsPollDone => Task::none(),
            Message::ProfileFieldChanged { field, value } => {
                self.account.editor.set(field, value);
                Task::none()
            }
            Message::ProfileSave => self.save_profile(),
            Message::ProfileSaved(result) => {
                self.account.editor.saving = false;
                match result {
                    Ok(()) => {
                        // A página do editor continua aberta: salvar agora
                        // confirma no lugar (aviso) e o usuário sai pelo
                        // "Voltar ao perfil" — antes o form fechava sozinho.
                        self.account.editor.error = None;
                        self.account.editor.notice =
                            Some(self.i18n.t("native.profile.updated").to_string());
                        self.account.notice = None;
                        self.account_profile_task()
                    }
                    Err(error) => {
                        self.account.editor.error = Some(account_error_text(&self.i18n, &error));
                        Task::none()
                    }
                }
            }
            Message::AvatarPathChanged(path) => {
                self.account.editor.avatar_path = path;
                Task::none()
            }
            Message::AvatarUpload => self.start_avatar_upload(),
            Message::AvatarUploadDone(result) => {
                self.account.editor.uploading_avatar = false;
                match result {
                    Ok(url) => {
                        self.account.editor.avatar_path.clear();
                        self.account.editor.notice =
                            Some(self.i18n.t("native.profile.avatar_updated").to_string());
                        self.account.avatar_url = Some(url.clone());
                        if let Some(profile) = self.account.profile.as_mut() {
                            profile.avatar_url = Some(url.clone());
                        }
                        self.avatar_load_task(url)
                    }
                    Err(error) => {
                        self.account.editor.error = Some(account_error_text(&self.i18n, &error));
                        Task::none()
                    }
                }
            }
            Message::ProfileImageCleared(kind) => self.clear_profile_image(kind),
            Message::ProfileImageClearedDone { kind, result } => {
                *self.account.editor.image_upload_busy_mut(kind) = false;
                match result {
                    Ok(()) => {
                        *self.account.editor.image_url_mut(kind) = String::new();
                        if let Some(profile) = self.account.profile.as_mut() {
                            match kind {
                                ProfileImageKind::Background => profile.background_url = None,
                                ProfileImageKind::Banner => profile.banner_url = None,
                            }
                        }
                        self.account.editor.error = None;
                        self.account.editor.notice =
                            Some(self.i18n.t("native.profile.image_removed").to_string());
                    }
                    Err(error) => {
                        // O valor publicado continua sendo o do servidor.
                        let atual = self
                            .account
                            .profile
                            .as_ref()
                            .map(|profile| match kind {
                                ProfileImageKind::Background => {
                                    profile.background_url.clone().unwrap_or_default()
                                }
                                ProfileImageKind::Banner => {
                                    profile.banner_url.clone().unwrap_or_default()
                                }
                            })
                            .unwrap_or_default();
                        *self.account.editor.image_url_mut(kind) = atual;
                        self.account.editor.error = Some(account_error_text(&self.i18n, &error));
                    }
                }
                Task::none()
            }
            Message::PickRequested(purpose) => self.request_pick(purpose),
            Message::PickDone { purpose, result } => self.finish_pick(purpose, result),
            Message::ProfileImageUploadDone { kind, result } => {
                *self.account.editor.image_upload_busy_mut(kind) = false;
                match result {
                    Ok(url) => {
                        *self.account.editor.image_url_mut(kind) = url.clone();
                        if let Some(profile) = self.account.profile.as_mut() {
                            match kind {
                                ProfileImageKind::Background => {
                                    profile.background_url = Some(url.clone());
                                }
                                ProfileImageKind::Banner => {
                                    profile.banner_url = Some(url.clone());
                                }
                            }
                        }
                        self.account.editor.error = None;
                        self.account.editor.notice =
                            Some(self.i18n.t(kind.success_key()).to_string());
                        // Prévia do editor: a URL recém-subida só aparece se
                        // entrar no cache de imagens.
                        return self.cover_tasks(vec![url]);
                    }
                    Err(error) => {
                        self.account.editor.error = Some(account_error_text(&self.i18n, &error));
                        Task::none()
                    }
                }
            }
            Message::FriendSearchChanged(query) => {
                self.friends.search_query = query;
                self.friends.search_requested=None;
                self.friends.search_results=None;
                if self.friends.search_query.trim().is_empty() {
                    self.friends.search_results = None;
                    self.friends.search_status = LoadStatus::Idle;
                    return Task::none();
                }
                self.friends.search_status=LoadStatus::Loading;
                let query=self.friends.search_query.trim().to_string();
                Task::perform(async move {tokio::time::sleep(Duration::from_millis(300)).await;query},Message::FriendSearchReady)
            }
            Message::FriendSearchSubmitted => self.submit_friend_search(),
            Message::FriendSearchReady(query) => {
                if self.friends.search_query.trim()!=query || self.friends.search_requested.as_deref()==Some(query.as_str()) {return Task::none();}
                self.submit_friend_search()
            }
            Message::FriendSearchDone { query, result } => {
                if !self.account.is_signed_in() || self.friends.search_query.trim()!=query {return Task::none();}
                match result {
                    Ok(results) => {
                        let urls = results.iter().filter_map(|friend|friend.avatar_url.clone()).collect();
                        self.friends.search_results = Some(results);
                        self.friends.search_status = LoadStatus::Ready;
                        self.friends.action_error = None;
                        let covers=self.cover_tasks(urls);
                        let avatars=self.ensure_friend_avatars();
                        return Task::batch([covers,avatars]);
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] amigos (busca): {error}");
                        self.friends.search_results = None;
                        self.friends.search_status = LoadStatus::Error(error.clone());
                        self.friends.action_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::FriendActionRequested { id, action } => self.request_friend_action(id, action),
            Message::FriendActionDone { id, action, result } => {
                self.friends.action_busy = None;
                match result {
                    Ok(()) => {
                        self.friends.action_error = None;
                        self.friends.notice = Some(action.success_message(&self.i18n).to_string());
                        if action == FriendAction::Remove
                            && self
                                .friends
                                .profile
                                .as_ref()
                                .is_some_and(|profile| profile.id == id)
                        {
                            self.friends.profile = None;
                        }
                        let list = self.refresh_friends_list();
                        if action == FriendAction::Remove {self.friend_media.clear();}
                        let search = self.submit_friend_search();
                        Task::batch([list, search])
                    }
                    Err(error) => {
                        self.friends.action_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::FriendProfileRequested(id) => {
                self.friend_media.clear_profile();
                let is_friend = self.is_accepted_friend(&id);
                let person = match &self.friends.data {
                    Some(FriendsLoad::Ready(data)) => data.friends.iter().chain(&data.incoming).chain(&data.outgoing).find(|person|person.id==id),
                    _ => None,
                };
                let preview = person.map(|person|FriendProfile {username:person.username.clone(),display_name:person.display_name.clone(),avatar_url:person.avatar_url.clone(),..Default::default()})
                    .or_else(||self.friends.search_results.as_ref().and_then(|results|results.iter().find(|person|person.id==id)).map(|person|FriendProfile {username:person.username.clone(),display_name:person.display_name.clone(),avatar_url:person.avatar_url.clone(),..Default::default()}))
                    .unwrap_or_default();
                self.friends.profile = Some(FriendProfileState {
                    id: id.clone(),
                    is_friend,
                    status: LoadStatus::Loading,
                    profile: Some(preview),
                    error: None,
                    achievements_status: LoadStatus::Idle,
                    achievements: None,
                    achievements_error: None,
                    confirming_remove: false,
                });
                let backend = self.backend.clone();
                let request_id = id.clone();
                let profile_message_id = id.clone();
                let profile_task = Task::perform(
                    async move { backend.load_friend_profile(request_id).await },
                    move |result| Message::FriendProfileLoaded {
                        id: profile_message_id.clone(),
                        result,
                    },
                );
                profile_task
            }
            Message::FriendProfileLoaded { id, result } => {
                let Some(profile) = self.friends.profile.as_mut() else {
                    return Task::none();
                };
                if profile.id != id {
                    return Task::none();
                }
                match result {
                    Ok(mut friend) => {
                        if let Some(FriendsLoad::Ready(data))=&self.friends.data {
                            if let Some(person)=data.friends.iter().chain(&data.incoming).chain(&data.outgoing).find(|person|person.id==id) {
                                if friend.username.is_empty() {friend.username=person.username.clone();}
                                if friend.display_name.is_none() {friend.display_name=person.display_name.clone();}
                                if friend.avatar_url.is_none() {friend.avatar_url=person.avatar_url.clone();}
                            }
                        }
                        // Só a arte vem da biblioteca local: jogos e minutos
                        // pertencem exclusivamente ao perfil que foi aberto.
                        for game in &mut friend.library {
                            if let Some(local) = self.library.games.iter().find(|local|local.id==game.id) {
                                if local.cover_url.is_some() {game.cover_url=local.cover_url.clone();}
                            }
                        }
                        friend.library.sort_by(|a,b|b.playtime_minutes.cmp(&a.playtime_minutes));
                        let urls = friend.library.iter().flat_map(Game::image_urls)
                            .chain(friend.friend_list.iter().filter_map(|friend|friend.avatar_url.clone()))
                            .chain([friend.avatar_url.clone(),friend.banner_url.clone(),friend.background_url.clone()].into_iter().flatten()).collect();
                        profile.profile = Some(friend);
                        profile.status = LoadStatus::Ready;
                        profile.error = None;
                        let covers=self.cover_tasks(urls);
                        let media=self.ensure_friend_media();
                        let avatars=self.ensure_friend_avatars();
                        return Task::batch([covers,media,avatars]);
                    }
                    Err(error) => {
                        eprintln!("[arcadia-native] amigos (perfil): {error}");
                        profile.status = LoadStatus::Error(error.clone());
                        profile.error = Some(error);
                    }
                }
                Task::none()
            }
            Message::FriendAchievementsLoaded { id, result } => {
                let icons = {
                    let Some(profile) = self.friends.profile.as_mut() else {
                        return Task::none();
                    };
                    if profile.id != id {
                        return Task::none();
                    }
                    match result {
                        Ok(achievements) => {
                            let icons: Vec<String> = achievements
                                .items
                                .iter()
                                .filter_map(|item| item.icon.clone())
                                .take(64)
                                .collect();
                            profile.achievements = Some(achievements);
                            profile.achievements_status = LoadStatus::Ready;
                            profile.achievements_error = None;
                            icons
                        }
                        Err(error) => {
                            eprintln!("[arcadia-native] amigos (conquistas): {error}");
                            profile.achievements = None;
                            profile.achievements_status = LoadStatus::Error(error.clone());
                            profile.achievements_error = Some(error);
                            Vec::new()
                        }
                    }
                };
                self.cover_tasks(icons)
            }
            Message::FriendProfileClosed => {
                self.friends.profile = None;
                self.friend_media.clear_profile();
                self.ensure_friend_avatars()
            }
            Message::FriendAvatarLoaded {url,media} => {
                self.friend_media.clear_in_flight(&url);
                if !self.friend_avatar_urls().contains(&url) {return Task::none();}
                if let Some(media)=media {self.friend_media.insert_decoded(&url,media,crate::animated_cards::MediaGroup::Avatar);}
                else {eprintln!("[arcadia-native] avatar de amigo: animação indisponível");}
                Task::none()
            }
            Message::FriendMediaLoaded { id, url, media } => {
                if !self.friends.profile.as_ref().is_some_and(|profile|profile.id==id) {return Task::none();}
                self.friend_media.clear_in_flight(&url);
                if let Some(media)=media {self.friend_media.insert_decoded(&url,media,crate::animated_cards::MediaGroup::Profile);}
                else {eprintln!("[arcadia-native] mídia do amigo: imagem indisponível");}
                Task::none()
            }
            Message::FriendRemoveConfirmation(confirming) => {
                if let Some(profile)=self.friends.profile.as_mut() {profile.confirming_remove=confirming;}
                Task::none()
            }
        }
    }

    fn account_status_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_account_status().await },
            Message::AccountStatusLoaded,
        )
    }

    fn account_profile_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_account_profile().await },
            Message::AccountProfileLoaded,
        )
    }

    /// Baixa avatar, banner e fundo do perfil para o pool animado.
    ///
    /// A web serve os três como GIF/WebP animado. O `image` decodifica todos
    /// os quadros em worker, sem bloquear o update do iced. O download próprio
    /// preserva a animação, ao contrário de `fetch_cover`, que só entrega o
    /// primeiro quadro como `Handle`.
    ///
    /// A sidebar mostra o avatar o tempo todo, então isto roda no boot e em
    /// toda chegada de perfil, não só ao abrir a tela de perfil.
    fn ensure_profile_media(&mut self) -> Task<Message> {
        // Avatar: o da conta quando existe, senão o local do config — o mesmo
        // `||` do Sidebar.tsx.
        let avatar = self
            .account
            .avatar_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .or(self
                .account
                .fallback_avatar_url
                .as_deref()
                .filter(|url| !url.trim().is_empty()))
            .map(str::to_string);

        let mut urls: Vec<String> = Vec::new();
        if let Some(url) = avatar {
            urls.push(url);
        }
        if let Some(profile) = self.account.profile.as_ref() {
            for url in [profile.banner_url.as_deref(), profile.background_url.as_deref()]
                .into_iter()
                .flatten()
                .filter(|url| !url.trim().is_empty())
            {
                urls.push(url.to_string());
            }
        }

        let mut tasks = Vec::new();
        for url in urls {
            if !self.animated_cards.wants(&url) {
                continue;
            }
            self.animated_cards.mark_in_flight(&url);
            // Vídeo (cards da Steam) é aberto direto pelo GStreamer; o perfil
            // é sempre imagem/GIF, então o caminho aqui é o download.
            let backend = self.backend.clone();
            let message_url = url.clone();
            tasks.push(Task::perform(
                async move {
                    let bytes = backend.fetch_media_bytes(url).await?;
                    crate::animated_cards::decode_media(bytes).await
                },
                move |media| Message::AnimatedMediaLoaded {
                    url: message_url,
                    media,
                },
            ));
        }
        Task::batch(tasks)
    }

    fn friend_avatar_urls(&self) -> BTreeSet<String> {
        let mut urls=BTreeSet::new();
        if let Some(FriendsLoad::Ready(data))=&self.friends.data {
            urls.extend(data.friends.iter().chain(&data.incoming).chain(&data.outgoing).filter_map(|person|person.avatar_url.clone()));
        }
        if let Some(results)=&self.friends.search_results {urls.extend(results.iter().filter_map(|person|person.avatar_url.clone()));}
        if let Some(profile)=self.friends.profile.as_ref().and_then(|state|state.profile.as_ref()) {
            urls.extend(profile.friend_list.iter().filter_map(|person|person.avatar_url.clone()));
            urls.extend(profile.avatar_url.clone());
        }
        urls
    }

    pub fn friend_avatar_texture(&self,url:&str)->Option<&Handle> {
        self.friend_media.texture(url).or_else(||self.animated_cards.texture(url)).or_else(||self.covers.ready(Some(url)))
    }

    fn ensure_friend_avatars(&mut self) -> Task<Message> {
        let urls=self.friend_avatar_urls();
        self.friend_media.retain_avatars(&urls);
        let mut tasks=Vec::new();
        for url in urls {
            if !self.friend_media.wants(&url) || self.animated_cards.texture(&url).is_some() {continue;}
            self.friend_media.mark_in_flight(&url);
            let backend=self.backend.clone();let request=url.clone();
            tasks.push(Task::perform(async move {let bytes=backend.fetch_media_bytes(request).await?;crate::animated_cards::decode_avatar(bytes).await},
                move |media|Message::FriendAvatarLoaded{url,media}));
        }
        Task::batch(tasks)
    }

    fn ensure_friend_media(&mut self) -> Task<Message> {
        let Some(state)=self.friends.profile.as_ref() else {return Task::none();};
        let Some(profile)=state.profile.as_ref() else {return Task::none();};
        let id=state.id.clone();
        let urls:Vec<_>=[profile.banner_url.clone(),profile.background_url.clone()].into_iter().flatten().collect();
        let mut tasks=Vec::new();
        for url in urls {
            if !self.friend_media.wants(&url) || self.animated_cards.texture(&url).is_some() {continue;}
            self.friend_media.mark_in_flight(&url);
            let backend=self.backend.clone();let request=url.clone();let id=id.clone();
            tasks.push(Task::perform(async move {let bytes=backend.fetch_media_bytes(request).await?;crate::animated_cards::decode_media(bytes).await},
                move |media|Message::FriendMediaLoaded{id,url,media}));
        }
        Task::batch(tasks)
    }

    fn profile_stats_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_profile_stats().await },
            Message::ProfileStatsLoaded,
        )
    }

    fn avatar_load_task(&self, url: String) -> Task<Message> {
        let backend = self.backend.clone();
        let request_url = url.clone();
        // O erro do `avatar:load` precisa aparecer: era engolido pelo `.ok()` e
        // a sidebar ficava na inicial para sempre, sem nada no console.
        Task::perform(
            async move {
                let result = backend.load_avatar(request_url).await;
                if let Err(error) = result.as_ref() {
                    eprintln!("[arcadia-native] avatar:load falhou: {error}");
                }
                result.ok()
            },
            move |handle| Message::AccountAvatarLoaded { url, handle },
        )
    }

    /// Garante que o fallback local esteja em `account.fallback_avatar_url` e
    /// dispara o download quando a conta ainda não tem avatar de fato.
    fn sync_fallback_avatar(&mut self) -> Task<Message> {
        let Some(url) = self
            .settings
            .config
            .as_ref()
            .and_then(|config| config.local_avatar_url.clone())
        else {
            // Sem `profile.avatar` no config não há reserva: a sidebar fica na
            // inicial de propósito. O log existe para "só a letra" nunca mais
            // ser um mistério sem causa.
            eprintln!(
                "[arcadia-native] avatar: sem avatar da conta e sem profile.avatar no config — sidebar na inicial"
            );
            return Task::none();
        };
        if self.account.avatar_url.is_some() && self.account.avatar.is_some() {
            // A conta já tem avatar pintado; o local fica só de reserva.
            self.account.fallback_avatar_url = Some(url);
            return Task::none();
        }
        if self.account.fallback_avatar_url.as_deref() == Some(url.as_str())
            && self.account.avatar.is_some()
        {
            return Task::none();
        }
        self.account.fallback_avatar_url = Some(url.clone());
        let avatar_task = self.avatar_load_task(url);
        let media_task = self.ensure_profile_media();
        Task::batch([avatar_task, media_task])
    }

    /// Usado quando `account:profile` chega sem avatar utilizável.
    fn fallback_avatar_task(&mut self) -> Task<Message> {
        self.sync_fallback_avatar()
    }

    fn start_avatar_upload(&mut self) -> Task<Message> {
        if self.account.editor.uploading_avatar {
            return Task::none();
        }
        let path = self.account.editor.avatar_path.trim().to_string();
        if path.is_empty() {
            self.account.editor.error = Some(
                self.i18n
                    .t("native.profile.avatar_path_required")
                    .to_string(),
            );
            return Task::none();
        }
        self.account.editor.uploading_avatar = true;
        self.account.editor.error = None;
        self.account.editor.notice = None;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.set_account_avatar(path).await },
            Message::AvatarUploadDone,
        )
    }

    fn start_image_upload(&mut self, kind: ProfileImageKind, path: PathBuf) -> Task<Message> {
        if *self.account.editor.image_upload_busy_mut(kind) {
            return Task::none();
        }
        *self.account.editor.image_upload_busy_mut(kind) = true;
        self.account.editor.error = None;
        self.account.editor.notice = None;
        let backend = self.backend.clone();
        let path = path.to_string_lossy().into_owned();
        let ipc_kind = kind.ipc_kind().to_string();
        Task::perform(
            async move { backend.set_account_background(path, &ipc_kind).await },
            move |result| Message::ProfileImageUploadDone { kind, result },
        )
    }

    /// Remove o banner/fundo publicado: `account:updateProfile` aceita os dois
    /// campos como `null` e o merge do perfil online deixa de trazer a imagem.
    fn clear_profile_image(&mut self, kind: ProfileImageKind) -> Task<Message> {
        if *self.account.editor.image_upload_busy_mut(kind) {
            return Task::none();
        }
        if !self.account.is_signed_in() {
            self.account.open_auth(false);
            return self.auth_focus_task();
        }
        *self.account.editor.image_upload_busy_mut(kind) = true;
        self.account.editor.error = None;
        self.account.editor.notice = None;
        let backend = self.backend.clone();
        let patch = json!({ kind.ipc_field(): Value::Null });
        Task::perform(
            async move { backend.update_account_profile(patch).await },
            move |result| Message::ProfileImageClearedDone { kind, result },
        )
    }

    fn request_pick(&mut self, purpose: PickPurpose) -> Task<Message> {
        match purpose.target() {
            PickTarget::SourceCatalog => {
                if self.sources.busy {
                    return Task::none();
                }
                let PickPurpose::SourceCatalog { url } = &purpose else {
                    return Task::none();
                };
                if url.trim().is_empty() {
                    self.sources.action_error = Some(self.i18n.t("native.sources.url_required").to_string());
                    return Task::none();
                }
                self.sources.busy = true;
                self.sources.action_error = None;
            }
            PickTarget::Avatar | PickTarget::Background | PickTarget::Banner => {
                if !self.account.is_signed_in() || self.account.editor.pick_busy.is_some() {
                    return Task::none();
                }
                self.account.editor.pick_busy = Some(purpose.clone());
                self.account.editor.error = None;
            }
            PickTarget::GameRom | PickTarget::GameCore => {
                if !self.launch_options.open
                    || purpose.game_id() != Some(self.launch_options.game_id.as_str())
                    || self.launch_options.pick_busy.is_some()
                {
                    return Task::none();
                }
                let matches_emulator =
                    self.launch_options
                        .settings
                        .as_ref()
                        .is_some_and(|settings| {
                            purpose.emulator_id() == settings.emulator_id.as_deref()
                        });
                if !matches_emulator {
                    return Task::none();
                }
                self.launch_options.pick_busy = Some(purpose.clone());
                self.launch_options.error = None;
            }
            PickTarget::Artwork => {
                let Some(metadata) = self.game_metadata.as_mut() else {
                    return Task::none();
                };
                if purpose.game_id() != Some(metadata.game_id.as_str())
                    || metadata.pick_busy.is_some()
                    || metadata.saving
                {
                    return Task::none();
                }
                metadata.pick_busy = Some(purpose.clone());
                metadata.error = None;
            }
            PickTarget::StoreDownloadFolder => {
                let Some(detail) = self.store.detail.as_mut() else { return Task::none() };
                if detail.install_busy || detail.downloads.pick_busy || !detail.downloads.open
                    || purpose.game_id() != Some(detail.game.appid.as_str())
                    || !matches!(&purpose, PickPurpose::StoreDownloadFolder { generation, .. } if *generation == detail.downloads.generation)
                {
                    return Task::none();
                }
                detail.downloads.pick_busy = true;
                detail.downloads.error = None;
            }
            _ => {
                let Some(editor) = self.emulation.editor.as_mut() else {
                    return Task::none();
                };
                if editor.pick_busy.is_some()
                    || purpose.emulator_id() != Some(editor.emulator_id.as_str())
                {
                    return Task::none();
                }
                editor.pick_busy = Some(purpose.clone());
                editor.error = None;
            }
        }

        let backend = self.backend.clone();
        let request = purpose.clone();
        Task::perform(
            async move { backend.pick_path(request).await },
            move |result| Message::PickDone { purpose, result },
        )
    }

    fn finish_pick(
        &mut self,
        purpose: PickPurpose,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        match purpose.target() {
            PickTarget::SourceCatalog => {
                let PickPurpose::SourceCatalog { url } = purpose else {
                    return Task::none();
                };
                match result {
                    Ok(Some(path)) => {
                        let backend = self.backend.clone();
                        Task::perform(
                            async move { backend.import_source_file(url, path).await },
                            Message::SourcesActionDone,
                        )
                    }
                    Ok(None) => {
                        self.sources.busy = false;
                        Task::none()
                    }
                    Err(error) => {
                        self.sources.busy = false;
                        self.sources.action_error = Some(picker_error_text(&self.i18n, &error));
                        Task::none()
                    }
                }
            }
            PickTarget::StoreDownloadFolder => {
                let Some(detail) = self.store.detail.as_mut().filter(|detail| {
                    purpose.game_id() == Some(detail.game.appid.as_str())
                        && matches!(&purpose, PickPurpose::StoreDownloadFolder { generation, .. } if *generation == detail.downloads.generation)
                }) else { return Task::none() };
                detail.downloads.pick_busy = false;
                match result {
                    Ok(Some(path)) => detail.downloads.path = path.to_string_lossy().into_owned(),
                    Ok(None) => {}
                    Err(error) => detail.downloads.error = Some(picker_error_text(&self.i18n, &error)),
                }
                Task::none()
            }
            PickTarget::Avatar => {
                self.account.editor.pick_busy = None;
                match result {
                    Err(error) => {
                        self.account.editor.error = Some(picker_error_text(&self.i18n, &error));
                        Task::none()
                    }
                    Ok(None) => Task::none(),
                    Ok(Some(path)) => {
                        self.account.editor.avatar_path = path.to_string_lossy().into_owned();
                        self.start_avatar_upload()
                    }
                }
            }
            PickTarget::Background | PickTarget::Banner => {
                self.account.editor.pick_busy = None;
                let Some(kind) = ProfileImageKind::from_purpose(&purpose) else {
                    return Task::none();
                };
                match result {
                    Err(error) => {
                        self.account.editor.error = Some(picker_error_text(&self.i18n, &error));
                        Task::none()
                    }
                    Ok(None) => Task::none(),
                    Ok(Some(path)) => self.start_image_upload(kind, path),
                }
            }
            PickTarget::GameRom | PickTarget::GameCore => {
                if self.launch_options.pick_busy.as_ref() == Some(&purpose) {
                    self.launch_options.pick_busy = None;
                }
                match result {
                    Err(error) => {
                        if self.launch_options.open {
                            self.launch_options.error = Some(picker_error_text(&self.i18n, &error));
                        }
                    }
                    Ok(None) => {}
                    Ok(Some(path)) => {
                        self.launch_options.apply_pick(&purpose, path);
                    }
                }
                Task::none()
            }
            PickTarget::Artwork => {
                let PickPurpose::Artwork { game_id, kind } = purpose else {
                    return Task::none();
                };
                let Some(metadata) = self.game_metadata.as_mut() else {
                    return Task::none();
                };
                if metadata.game_id != game_id {
                    return Task::none();
                }
                metadata.pick_busy = None;
                match result {
                    Err(error) => {
                        metadata.error = Some(picker_error_text(&self.i18n, &error));
                    }
                    Ok(None) => {}
                    Ok(Some(path)) => metadata.set_art(kind, path),
                }
                Task::none()
            }
            _ => {
                let Some(editor) = self.emulation.editor.as_mut() else {
                    return Task::none();
                };
                if editor.pick_busy.as_ref() == Some(&purpose) {
                    editor.pick_busy = None;
                }
                match result {
                    Err(error) => {
                        editor.error = Some(picker_error_text(&self.i18n, &error));
                    }
                    Ok(None) => {}
                    Ok(Some(path)) => match editor.apply_pick(&purpose, path) {
                        Ok(true) => editor.error = None,
                        Ok(false) => {}
                        Err(error) => {
                            editor.error = Some(rom_folder_error_text(&self.i18n, error));
                        }
                    },
                }
                Task::none()
            }
        }
    }

    fn auth_focus_task(&self) -> Task<Message> {
        let id = if self.account.auth.mode == AuthMode::SignUp {
            iced::widget::Id::new(views::auth::EMAIL_ID)
        } else {
            iced::widget::Id::new(views::auth::USERNAME_ID)
        };
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(id))
    }

    fn submit_auth(&mut self) -> Task<Message> {
        if self.account.auth.sending {
            return Task::none();
        }
        let username = self.account.auth.username.trim().to_string();
        let email = self.account.auth.email.trim().to_string();
        let password = self.account.auth.password.clone();

        let error = match self.account.auth.mode {
            AuthMode::SignIn => {
                if username.is_empty() {
                    Some(self.i18n.t("native.auth.username_required").to_string())
                } else if password.is_empty() {
                    Some(self.i18n.t("native.auth.password_required").to_string())
                } else {
                    None
                }
            }
            AuthMode::SignUp => {
                if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
                    Some(self.i18n.t("account.erro_email").to_string())
                } else if username.is_empty() {
                    Some(self.i18n.t("native.auth.username_required").to_string())
                } else if password.chars().count() < 6 {
                    Some(self.i18n.t("account.erro_senha_curta").to_string())
                } else {
                    None
                }
            }
        };
        if let Some(error) = error {
            self.account.auth.error = Some(error);
            return Task::none();
        }

        self.account.auth.sending = true;
        self.account.auth.error = None;
        let backend = self.backend.clone();
        match self.account.auth.mode {
            AuthMode::SignIn => Task::perform(
                async move { backend.sign_in(username, password).await },
                Message::AuthDone,
            ),
            AuthMode::SignUp => Task::perform(
                async move { backend.sign_up(email, username, password).await },
                Message::AuthDone,
            ),
        }
    }

    fn account_key(&mut self, event: keyboard::Event) -> Task<Message> {
        if !self.account.auth.open {
            return Task::none();
        }
        let keyboard::Event::KeyPressed { key, .. } = event else {
            return Task::none();
        };
        use keyboard::key::Named;
        match key {
            keyboard::Key::Named(Named::Escape) => {
                self.account.close_auth();
                Task::none()
            }
            keyboard::Key::Named(Named::Enter) => self.submit_auth(),
            _ => Task::none(),
        }
    }

    fn save_profile(&mut self) -> Task<Message> {
        if self.account.editor.saving {
            return Task::none();
        }
        if !self.account.is_signed_in() {
            self.account.open_auth(false);
            return self.auth_focus_task();
        }

        let editor = &self.account.editor;
        let showcase: Vec<String> = editor
            .showcase
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
        let patch = json!({
            "display_name": editor.display_name.trim(),
            "summary": editor.summary.trim(),
            "country": editor.country.trim(),
            "city": editor.city.trim(),
            "showcase": showcase,
            "background_url": editor.background_url.trim(),
            "banner_url": editor.banner_url.trim(),
        });
        self.account.editor.saving = true;
        self.account.editor.error = None;
        self.account.editor.notice = None;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.update_account_profile(patch).await },
            Message::ProfileSaved,
        )
    }

    fn refresh_friends_list(&mut self) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        self.friends.status = LoadStatus::Loading;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_friends(true).await },
            Message::FriendsLoaded,
        )
    }

    /// Amigo aceito na lista ou num resultado de busca; só eles liberam
    /// `friends:achievements`.
    fn is_accepted_friend(&self, id: &str) -> bool {
        let in_list = matches!(
            self.friends.data.as_ref(),
            Some(FriendsLoad::Ready(data)) if data.friends.iter().any(|friend| friend.id == id)
        );
        in_list
            || self.friends.search_results.as_ref().is_some_and(|results| {
                results
                    .iter()
                    .any(|result| result.id == id && result.status.as_deref() == Some("accepted"))
            })
    }

    fn submit_friend_search(&mut self) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let query = self.friends.search_query.trim().to_string();
        if query.is_empty() {
            self.friends.search_results = None;
            self.friends.search_status = LoadStatus::Idle;
            return Task::none();
        }
        self.friends.search_status = LoadStatus::Loading;
        self.friends.search_requested=Some(query.clone());
        let backend = self.backend.clone();
        let message_query=query.clone();
        Task::perform(
            async move { backend.search_friends(query).await },
            move |result| Message::FriendSearchDone {query:message_query,result},
        )
    }

    fn request_friend_action(&mut self, id: String, action: FriendAction) -> Task<Message> {
        if self.friends.action_busy.is_some() {
            return Task::none();
        }
        self.friends.action_busy = Some(id.clone());
        self.friends.action_error = None;
        self.friends.notice = None;
        let backend = self.backend.clone();
        let request_id = id.clone();
        Task::perform(
            async move {
                backend
                    .run_friend_action(action.channel(), request_id)
                    .await
            },
            move |result| Message::FriendActionDone { id, action, result },
        )
    }

    /// Entra no Big Picture guardando a tela desktop para restaurar ao sair.
    fn enter_big_picture(&mut self) -> Task<Message> {
        if matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        self.previous_screen = Some(self.screen.clone());
        // O pipeline de trailer do desktop para ao entrar; o painel do console
        // abre o trailer do jogo selecionado sob demanda.
        self.media.clear();
        self.game_media = None;
        self.reset_trailer_state();
        self.console_media_id = None;
        self.favorite_busy = None;
        self.screen = Screen::BigPicture;

        let count = self.library.visible_all().len();
        self.big_picture.enter(count);
        eprintln!("[arcadia-native] Big Picture: {count} jogo(s)");

        let urls = self.library.image_urls();
        // A logo do hero é uma imagem remota como qualquer outra: sem alguém
        // pedir, ela nunca entra no cache e o hero cai no título em texto. O
        // `.jpg` de capa que a biblioteca já enumera não cobre o `logo.png`.
        let mut urls = urls;
        for game in self.library.visible_all() {
            if let Some(logo) = crate::backend::game_logo_url(game) {
                if !urls.contains(&logo) {
                    urls.push(logo);
                }
            }
        }
        let covers = self.cover_tasks(urls);

        let splash = self.start_boot_splash();
        let mut tasks = vec![covers, splash];
        if let Some(window) = self.window {
            tasks.push(window::set_mode(window, window::Mode::Fullscreen));
            tasks.push(window::size(window).map(Message::WindowResized));
        }
        Task::batch(tasks)
    }

    /// Toca o `boot.mp4` ao entrar no Big Picture, como a `PS5Launcher` da UI
    /// web. O splash do desktop não existe: abrir o Arcadia não deve passar por
    /// uma tela de vídeo.
    fn start_boot_splash(&mut self) -> Task<Message> {
        if self.boot.active {
            return Task::none();
        }
        self.boot.active = true;
        self.boot.started = Instant::now();
        self.boot.path = None;
        self.boot.media.clear();
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_boot_video().await },
            Message::BootVideoReady,
        )
    }

    /// Volta ao desktop na tela em que o console foi aberto.
    fn exit_big_picture(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        self.dismiss_splash();
        let previous = self.previous_screen.take().unwrap_or(Screen::Library);
        self.big_picture.leave();
        self.media.clear();
        self.game_media = None;
        self.console_media_id = None;
        self.favorite_busy = None;
        self.screen = previous;
        eprintln!("[arcadia-native] Big Picture encerrado");

        let mut tasks = Vec::new();
        if let Some(window) = self.window {
            tasks.push(window::set_mode(window, window::Mode::Windowed));
        }

        // A ficha restaurada precisa do trailer de novo: ao entrar no console
        // o pipeline foi descartado junto com o `game_media`.
        if let Screen::Game(id) = self.screen.clone() {
            let game = self.library.game(&id).cloned();
            let urls = game.as_ref().map(Game::image_urls).unwrap_or_default();
            self.media.clear();
            self.game_media = None;
            self.reset_trailer_state();
            tasks.push(self.cover_tasks(urls));
            if let Some(game) = game {
                tasks.push(self.game_media_task(&game));
            }
        }

        Task::batch(tasks)
    }

    /// Fecha o seletor e devolve o trailer padrão (`game:sysinfo`) ao estado
    /// inicial sempre que a ficha deixa de ser a tela atual.
    fn reset_trailer_state(&mut self) {
        self.trailer_picker = TrailerPickerState::default();
        self.trailer_override = false;
    }

    fn current_game(&self) -> Option<&Game> {
        match &self.screen {
            Screen::Game(id) => self.library.game(id),
            _ => None,
        }
    }

    /// Abre o seletor com busca automática pelo título do jogo e consulta em
    /// paralelo se já existe trailer baixado (`trailer:path`).
    fn open_trailer_picker(&mut self) -> Task<Message> {
        let Some(game) = self.current_game().cloned() else {
            return Task::none();
        };

        self.trailer_picker = TrailerPickerState {
            open: true,
            game_id: game.id.clone(),
            game_title: game.title.clone(),
            query: game.title.clone(),
            status: LoadStatus::Loading,
            ..TrailerPickerState::default()
        };

        let backend = self.backend.clone();
        let path_id = game.id.clone();
        let message_game_id = path_id.clone();
        let path_task = Task::perform(
            async move { backend.trailer_path(path_id).await },
            move |result| Message::TrailerPathLoaded {
                game_id: message_game_id,
                result,
            },
        );

        let backend = self.backend.clone();
        let search_query = game.title;
        let message_query = search_query.clone();
        let search_task = Task::perform(
            async move { backend.search_trailers(search_query).await },
            move |result| Message::TrailerSearchDone {
                query: message_query,
                result,
            },
        );

        Task::batch([path_task, search_task])
    }

    fn submit_trailer_search(&mut self) -> Task<Message> {
        if !self.trailer_picker.open || self.trailer_picker.downloading.is_some() {
            return Task::none();
        }

        let query = self.trailer_picker.query.trim().to_string();
        if query.is_empty() {
            self.trailer_picker.results.clear();
            self.trailer_picker.status =
                LoadStatus::Error(self.i18n.t("native.game.trailer_search_empty").to_string());
            return Task::none();
        }

        self.trailer_picker.query = query.clone();
        self.trailer_picker.results.clear();
        self.trailer_picker.status = LoadStatus::Loading;
        self.trailer_picker.download_error = None;

        let backend = self.backend.clone();
        let message_query = query.clone();
        Task::perform(
            async move { backend.search_trailers(query).await },
            move |result| Message::TrailerSearchDone {
                query: message_query,
                result,
            },
        )
    }

    /// Troca o HLS da Steam pelo arquivo baixado escolhido explicitamente.
    fn open_local_trailer(&mut self, path: String) -> Task<Message> {
        self.trailer_override = true;
        match self.media.open_local(path) {
            Ok(()) => Task::none(),
            Err(error) => Task::done(Message::MediaError(error)),
        }
    }

    /// Cache público de fichas; não inclui progresso nem dados da conta.
    pub fn cached_store_info(&self, id: &str) -> Option<&StoreInfo> {
        self.store_info_cache.get(id).filter(|(at, _)| at.elapsed() < Duration::from_secs(600)).map(|(_, info)| info)
    }

    pub fn storefront_genres_loading(&self) -> bool {
        self.console_storefront.category >= 3 && !self.console_storefront.catalog
            && !self.detail_prefetching.is_empty()
    }

    fn cache_store_info(&mut self, id: &str, info: StoreInfo) {
        if self.store_info_cache.get(id).is_some_and(|(at, value)| at.elapsed() < Duration::from_secs(600) && value == &info) { return; }
        if self.store_info_cache.len() >= 64 && !self.store_info_cache.contains_key(id) {
            if let Some(oldest) = self.store_info_cache.iter().min_by_key(|(_, (at, _))| *at).map(|(id, _)| id.clone()) {
                self.store_info_cache.remove(&oldest);
            }
        }
        self.store_info_cache.insert(id.to_string(), (Instant::now(), info));
    }

    fn cache_game_media(&mut self, id: &str, media: GameMedia) {
        if self.game_media_cache.get(id).is_some_and(|(at, value)| at.elapsed() < Duration::from_secs(600) && value == &media) { return; }
        if media.about.is_none() && media.movies.is_empty() && media.screenshots.is_empty() && media.req_min.is_none() && media.req_rec.is_none() && media.overview == Default::default() { return; }
        if self.game_media_cache.len() >= 64 && !self.game_media_cache.contains_key(id) {
            if let Some(oldest) = self.game_media_cache.iter().min_by_key(|(_, (at, _))| *at).map(|(id, _)| id.clone()) {
                self.game_media_cache.remove(&oldest);
            }
        }
        self.game_media_cache.insert(id.to_string(), (Instant::now(), media));
    }

    fn prefetch_store(&mut self, game: StoreGame) -> Task<Message> {
        if self.store_info_cache.get(&game.appid).is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(600))
            || self.detail_prefetching.len() >= 2 { return Task::none(); }
        if !self.detail_prefetching.insert(format!("store:{}", game.appid)) { return Task::none(); }
        let backend = self.backend.clone();
        let appid = game.appid.clone();
        Task::perform(async move { backend.load_store_info(game.appid, game.title).await }, move |result| Message::PrefetchedStore { appid, result })
    }

    fn prefetch_game(&mut self, id: String) -> Task<Message> {
        if self.game_media_cache.get(&id).is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(600))
            || self.detail_prefetching.len() >= 2 { return Task::none(); }
        let Some(game) = self.library.game(&id).cloned() else { return Task::none(); };
        if !self.detail_prefetching.insert(format!("game:{id}")) { return Task::none(); }
        let backend = self.backend.clone();
        Task::perform(async move { backend.load_game_media(game.id, game.title).await }, move |media| Message::PrefetchedGame { id, media })
    }

    fn prefetch_store_front(&mut self) -> Task<Message> {
        let games: Vec<_> = if matches!(self.screen, Screen::BigPicture) {
            self.console_store_games().into_iter().take(2).cloned().collect()
        } else if let Some(search) = &self.store.search {
            search.result.as_ref().map(|r| r.games.iter().take(2).cloned().collect()).unwrap_or_default()
        } else {
            self.store.browse.games.iter().take(2).cloned().collect()
        };
        Task::batch(games.into_iter().map(|game| self.prefetch_store(game)))
    }

    fn prefetch_store_genres(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab() != ConsoleTab::Stores || self.console_storefront.category < 3 || self.console_storefront.catalog || self.store.search.is_some() || self.big_picture.store_detail_open() { return Task::none(); }
        let candidates: Vec<_> = self.console_storefront.feed.specials.iter()
            .chain(&self.console_storefront.feed.releases).chain(&self.console_storefront.feed.popular)
            .chain(&self.store.browse.games).take(48).cloned().collect();
        let mut tasks = Vec::new();
        for game in candidates {
            if self.detail_prefetching.len() >= 2 { break; }
            if self.cached_store_info(&game.appid).is_some()
                || self.console_storefront.genre_attempted.contains(&game.appid) { continue; }
            self.console_storefront.genre_attempted.insert(game.appid.clone());
            tasks.push(self.prefetch_store(game));
        }
        Task::batch(tasks)
    }

    fn open_game(&mut self, id: String) -> Task<Message> {
        self.game_menu = None;
        let game = self.library.game(&id).cloned();
        let urls = game.as_ref().map(Game::image_urls).unwrap_or_default();
        self.page_entered = Instant::now();
        self.page_direction = 1.0;
        self.screen = Screen::Game(id);
        // Troca de jogo nunca reaproveita o pipeline anterior.
        self.media.clear();
        self.game_media = None;
        self.selected_screenshot = 0;
        self.gallery_trailer = false;
        self.requirements_tab = RequirementsTab::default();
        self.reset_trailer_state();
        let covers = self.cover_tasks(urls);
        let cached = game.as_ref().and_then(|g| self.game_media_cache.get(&g.id))
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(600)).map(|(_, m)| m.clone());
        // O cache já tem a ficha, mas aplicá-la aqui abre o GStreamer no
        // mesmo frame do clique. A tela troca na hora; a mídia entra no
        // próximo ciclo do runtime.
        let media = if let Some(media) = cached {
            let game_id = game.as_ref().unwrap().id.clone();
            Task::perform(async move { media }, move |media| Message::MediaLoaded { game_id, media })
        } else {
            game.as_ref().map(|game| self.game_media_task(game)).unwrap_or_else(Task::none)
        };
        let panels = game
            .as_ref()
            .map(|game| self.game_panels_task(game))
            .unwrap_or_else(Task::none);
        let community = game
            .as_ref()
            .and_then(|game| game.steam_appid())
            .map(|appid| self.community_tasks(appid))
            .unwrap_or_else(Task::none);
        Task::batch([
            covers,
            media,
            panels,
            community,
            views::game_detail::scroll_to_top(),
        ])
    }

    /// Drena a fila do `NativeShell` e aplica launch, conquistas e downloads.
    ///
    /// A Subscription chama isto a cada ~150 ms; não há consulta ao processo.
    fn drain_launch_events(&mut self) -> Task<Message> {
        let events = self.backend.drain_ui_events();
        if events.is_empty() {
            return Task::none();
        }

        let was_running = self.launch.status == GameRunStatus::Running;
        let mut icon_tasks = Vec::new();
        for event in events {
            match event {
                UiEvent::LibraryChanged => self.library_dirty = true,
                UiEvent::Launch(event) => self.launch.apply_event(event, &self.i18n),
                UiEvent::Downloads { is_torrent, items } => {
                    self.downloads.apply_progress(is_torrent, items);
                }
                UiEvent::Achievement(achievement) => {
                    if !self.notifications_enabled {
                        continue;
                    }
                    if let Some(icon) = achievement.icon.clone() {
                        if self.covers.request(&icon) {
                            let backend = self.backend.clone();
                            let fetch_url = icon.clone();
                            let message_url = icon.clone();
                            icon_tasks.push(Task::perform(
                                async move { backend.fetch_cover(fetch_url).await },
                                move |loaded| Message::CoverLoaded {
                                    url: message_url,
                                    loaded,
                                },
                            ));
                        }
                    }
                    self.toasts.push(achievement);
                }
            }
        }

        let restore = if was_running && self.launch.status == GameRunStatus::Idle {
            self.restore_window_task()
        } else {
            Task::none()
        };
        Task::batch([restore, Task::batch(icon_tasks)])
    }

    fn dismiss_splash(&mut self) {
        if self.boot.active {
            self.page_entered = Instant::now();
            self.home_entered = Instant::now();
            self.home_hero_started = Instant::now();
        }
        self.boot.active = false;
        self.boot.media.clear();
    }

    /// Tick do splash: drena frames e fecha no fim, erro ou timeout.
    fn boot_tick(&mut self) {
        self.boot.media.tick();
        if self.boot.path.is_some() {
            match self.boot.media.status() {
                media::MediaStatus::Ended | media::MediaStatus::Error(_) => {
                    self.dismiss_splash();
                    return;
                }
                _ => {}
            }
        }
        // Sem arquivo, o `boot:video` ainda está em voo; o limite evita que um
        // IPC travado deixe o splash eterno. Com vídeo, o pipeline local ganha
        // mais folga para o pré-roll.
        let timeout = if self.boot.path.is_some() {
            Duration::from_secs(20)
        } else {
            Duration::from_secs(4)
        };
        if self.boot.started.elapsed() > timeout {
            eprintln!("[arcadia-native] splash de boot excedeu o tempo; seguindo para a UI");
            self.dismiss_splash();
        }
    }

    /// Inicia a checagem; `manual` abre o diálogo mesmo se a versão já foi
    /// avisada, como no botão da aba Sobre.
    fn start_update_check(&mut self, manual: bool) -> Task<Message> {
        if self.update.checking {
            return Task::none();
        }
        self.update.checking = true;
        self.update.last_error = None;
        self.update.last_motivo = None;
        self.update.available_version = None;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.check_updates(manual).await },
            move |result| Message::UpdateCheckDone { manual, result },
        )
    }

    fn finish_update_check(
        &mut self,
        manual: bool,
        result: Result<UpdateCheckReport, String>,
    ) -> Task<Message> {
        self.update.checking = false;
        self.update.last_check = Some(now_label());

        let report = match result {
            Ok(report) => report,
            Err(error) => {
                self.update.last_error = Some(error.clone());
                if manual {
                    let channel = self.update_channel();
                    self.update.dialog = Some(UpdateDialogState {
                        kind: UpdateDialogKind::Packaged,
                        channel,
                        git: None,
                        packaged: None,
                        stage: None,
                        error: Some(error),
                    });
                }
                return Task::none();
            }
        };

        if let Some(error) = report.check.erro.clone() {
            self.update.last_error = Some(error.clone());
            if manual {
                let channel = report.channel.clone();
                self.update.dialog = Some(UpdateDialogState {
                    kind: if report.channel == "fonte" {
                        UpdateDialogKind::Git
                    } else {
                        UpdateDialogKind::Packaged
                    },
                    channel: channel.clone(),
                    git: (report.channel == "fonte").then(|| report.check.clone()),
                    packaged: report.snapshot.packaged.clone(),
                    stage: None,
                    error: Some(error),
                });
                return Task::none();
            }
        }

        self.update.snapshot = Some(report.snapshot.clone());
        self.update.available_version = report.check.versao.clone().or_else(|| {
            report
                .snapshot
                .packaged
                .as_ref()
                .and_then(|state| state.versao_nova.clone())
        });
        self.update.last_motivo = report.check.motivo.clone();

        if report.check.disponivel {
            self.update.last_error = None;
            self.open_update_dialog(&report, manual);
            return Task::none();
        }

        // Manual também mostra o motivo (sem suporte/jogo rodando) e erros de
        // ação; a checagem automática silencia para não interromper o boot.
        if manual {
            if let Some(motivo) = report.check.motivo.as_deref() {
                if motivo != "jogo_rodando" || report.snapshot.packaged.is_some() {
                    self.open_update_dialog(&report, true);
                }
            }
        }
        Task::none()
    }

    fn update_channel(&self) -> String {
        self.update
            .snapshot
            .as_ref()
            .and_then(UpdateSnapshot::channel)
            .unwrap_or("sem_suporte")
            .to_string()
    }

    /// Monta o diálogo respeitando `jaAvisado`/dispensa e a preferência de
    /// changelog.
    fn open_update_dialog(&mut self, report: &UpdateCheckReport, manual: bool) {
        let is_git = report.channel == "fonte";
        let version = report
            .check
            .versao
            .clone()
            .or_else(|| {
                report
                    .snapshot
                    .packaged
                    .as_ref()
                    .and_then(|state| state.versao_nova.clone())
            })
            .unwrap_or_default();

        if !manual && self.update.dismissed_version.as_deref() == Some(version.as_str()) {
            return;
        }

        let packaged = report.snapshot.packaged.clone();
        if !is_git {
            if let Some(state) = packaged.as_ref() {
                if !manual && state.ja_avisado {
                    return;
                }
            }
        }

        let state = if is_git {
            None
        } else {
            packaged.clone().or_else(|| {
                Some(PackagedState {
                    canal: report.channel.clone(),
                    suportado: report.check.ok,
                    versao_atual: String::new(),
                    versao_nova: report.check.versao.clone(),
                    tamanho: report.check.tamanho,
                    fase: if report.check.disponivel {
                        PackagedPhase::Disponivel
                    } else if report.check.motivo.as_deref() == Some("canal_nao_suportado") {
                        PackagedPhase::SemSuporte
                    } else {
                        PackagedPhase::Erro
                    },
                    progresso: 0,
                    erro: report.check.erro.clone(),
                    erro_de_fundo: false,
                    erro_acao: None,
                    ja_avisado: false,
                    jogo_rodando: report
                        .check
                        .motivo
                        .as_deref()
                        .is_some_and(|motivo| motivo == "jogo_rodando"),
                })
            })
        };

        self.update.dialog = Some(UpdateDialogState {
            kind: if is_git {
                UpdateDialogKind::Git
            } else {
                UpdateDialogKind::Packaged
            },
            channel: report.channel.clone(),
            git: is_git.then(|| report.check.clone()),
            packaged: state,
            stage: None,
            error: None,
        });
    }

    fn dismiss_update_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.update.dialog.take() else {
            return Task::none();
        };
        if dialog.is_busy() {
            self.update.dialog = Some(dialog);
            return Task::none();
        }

        self.clear_update_backdrop();

        let version = dialog
            .packaged
            .as_ref()
            .and_then(|state| state.versao_nova.clone())
            .or_else(|| dialog.git.as_ref().and_then(|check| check.versao.clone()))
            .unwrap_or_default();
        self.update.dismissed_version = (!version.is_empty()).then_some(version.clone());

        // "Depois" no canal empacotado persiste `jaAvisado`, como na web.
        if dialog.kind == UpdateDialogKind::Packaged {
            if let Some(state) = self
                .update
                .snapshot
                .as_mut()
                .and_then(|snap| snap.packaged.as_mut())
            {
                state.ja_avisado = true;
            }
            if !version.is_empty() {
                let backend = self.backend.clone();
                return Task::perform(
                    async move { backend.mark_update_advised(version).await },
                    Message::UpdateAdvisedDone,
                );
            }
        }
        Task::none()
    }

    fn start_update_download(&mut self) -> Task<Message> {
        let backend = self.backend.clone();
        let Some(dialog) = self.update.dialog.as_mut() else {
            return Task::none();
        };
        if dialog.is_busy() {
            return Task::none();
        }

        match dialog.kind {
            UpdateDialogKind::Git => {
                let deps = dialog
                    .git
                    .as_ref()
                    .map(|check| check.deps_mudaram)
                    .unwrap_or(false);
                dialog.stage = Some(UpdateStage::Applying);
                dialog.error = None;
                Task::perform(
                    async move { backend.apply_git_update(deps).await },
                    move |result| Message::UpdateActionDone {
                        action: UpdateAction::ApplyGit,
                        result,
                    },
                )
            }
            UpdateDialogKind::Packaged => {
                dialog.stage = Some(UpdateStage::Downloading);
                dialog.error = None;
                Task::perform(
                    async move { backend.download_packaged_update().await },
                    move |result| Message::UpdateActionDone {
                        action: UpdateAction::Download,
                        result,
                    },
                )
            }
        }
    }

    fn start_update_install(&mut self) -> Task<Message> {
        let backend = self.backend.clone();
        let Some(dialog) = self.update.dialog.as_mut() else {
            return Task::none();
        };
        if dialog.is_busy() {
            return Task::none();
        }
        let Some(state) = dialog.packaged.as_ref() else {
            return Task::none();
        };
        if state.fase != PackagedPhase::Pronto || !state.suportado {
            return Task::none();
        }

        dialog.stage = Some(UpdateStage::Installing);
        dialog.error = None;
        Task::perform(
            async move { backend.install_packaged_update().await },
            move |result| Message::UpdateActionDone {
                action: UpdateAction::Install,
                result,
            },
        )
    }

    fn retry_update(&mut self) -> Task<Message> {
        let Some(dialog) = self.update.dialog.as_ref() else {
            return Task::none();
        };
        if dialog.is_busy() {
            return Task::none();
        }
        // Erro de instalação/cache volta pelo check manual, como o "tentar" da
        // web; erro de download repete a ação explícita.
        let retry_download = dialog
            .packaged
            .as_ref()
            .is_some_and(|state| state.erro_acao.as_deref() == Some("baixar"));
        if retry_download {
            self.start_update_download()
        } else {
            self.start_update_check(true)
        }
    }

    fn finish_update_action(
        &mut self,
        action: UpdateAction,
        result: Result<UpdateActionResult, String>,
    ) -> Task<Message> {
        let backend = self.backend.clone();
        let fallback = self.i18n.t("update.erro_generico").to_string();
        let Some(dialog) = self.update.dialog.as_mut() else {
            return Task::none();
        };

        match result {
            Ok(result) if result.ok => match action {
                UpdateAction::Download => {
                    dialog.stage = None;
                    dialog.error = None;
                    if let Some(state) = dialog.packaged.as_mut() {
                        state.fase = PackagedPhase::Pronto;
                        state.progresso = 100;
                    }
                    Task::perform(
                        async move { backend.load_packaged_state().await },
                        Message::UpdateSnapshotPolled,
                    )
                }
                UpdateAction::Install => {
                    dialog.stage = Some(UpdateStage::Restarting);
                    dialog.error = None;
                    Task::none()
                }
                UpdateAction::ApplyGit => {
                    dialog.stage = Some(UpdateStage::Restarting);
                    dialog.error = None;
                    Task::none()
                }
            },
            Ok(result) => {
                let message = result.erro.unwrap_or(fallback);
                dialog.stage = None;
                dialog.error = Some(message.clone());
                if action == UpdateAction::Download {
                    if let Some(state) = dialog.packaged.as_mut() {
                        state.fase = PackagedPhase::Erro;
                        state.erro = Some(message);
                        state.erro_acao = Some("baixar".to_string());
                    }
                } else if action == UpdateAction::Install {
                    if let Some(state) = dialog.packaged.as_mut() {
                        state.fase = PackagedPhase::Erro;
                        state.erro = Some(message);
                        state.erro_acao = Some("instalar".to_string());
                    }
                }
                Task::none()
            }
            Err(error) => {
                dialog.stage = None;
                dialog.error = Some(error.clone());
                if action == UpdateAction::Download {
                    if let Some(state) = dialog.packaged.as_mut() {
                        state.fase = PackagedPhase::Erro;
                        state.erro = Some(error);
                        state.erro_acao = Some("baixar".to_string());
                    }
                }
                Task::none()
            }
        }
    }

    /// Uma consulta de snapshot por vez enquanto o diálogo está ocupado.
    fn poll_update_snapshot(&mut self) -> Task<Message> {
        let should_poll =
            self.update.dialog.as_ref().is_some_and(|dialog| {
                dialog.is_busy() && dialog.kind == UpdateDialogKind::Packaged
            });
        if !should_poll || self.update.poll_in_flight {
            return Task::none();
        }
        self.update.poll_in_flight = true;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_packaged_state().await },
            Message::UpdateSnapshotPolled,
        )
    }

    fn apply_polled_snapshot(&mut self, result: Result<PackagedState, String>) {
        self.update.poll_in_flight = false;
        let Ok(state) = result else {
            return;
        };
        if let Some(snapshot) = self.update.snapshot.as_mut() {
            snapshot.packaged = Some(state.clone());
        } else {
            self.update.snapshot = Some(UpdateSnapshot {
                packaged: Some(state.clone()),
                ..UpdateSnapshot::default()
            });
        }

        let Some(dialog) = self.update.dialog.as_mut() else {
            return;
        };
        if dialog.kind != UpdateDialogKind::Packaged {
            return;
        }
        dialog.packaged = Some(state.clone());
        if !state.fase.is_busy() {
            dialog.stage = None;
            if state.fase == PackagedPhase::Erro {
                dialog.error = state.erro.clone();
            }
        } else if let Some(error) = state.erro.clone() {
            dialog.error = Some(error);
        }
    }

    fn current_account_id(&self) -> Option<String> {
        self.account.session.as_ref().map(|session| session.id.clone())
    }

    fn menu_instance_matches(&self, game_id: &str, instance: u64) -> bool {
        self.game_menu
            .as_ref()
            .is_some_and(|menu| menu.game_id == game_id && menu.instance == instance)
    }

    fn set_menu_notice(&mut self, game_id: &str, notice: MenuNotice) {
        if let Some(menu) = self.game_menu.as_mut().filter(|menu| menu.game_id == game_id) {
            menu.notice = Some(notice);
        }
    }

    fn game_menu_action(&mut self, action: MenuAction) -> Task<Message> {
        let Some(menu) = self.game_menu.as_ref() else {
            return Task::none();
        };
        let game_id = menu.game_id.clone();
        let instance = menu.instance;
        let Some(game) = self.library.game(&game_id).cloned() else {
            self.game_menu = None;
            return Task::none();
        };

        match action {
            MenuAction::Primary => self.menu_primary(game, instance),
            MenuAction::Details => {
                self.game_menu = None;
                self.open_game(game_id)
            }
            MenuAction::Settings => {
                self.game_menu = None;
                self.open_launch_options(game_id)
            }
            MenuAction::Logs => {
                let backend = self.backend.clone();
                let message_id = game_id.clone();
                let message_instance = instance;
                Task::perform(
                    async move { backend.open_game_log(game_id).await },
                    move |result| Message::GameMenuLogsDone {
                        game_id: message_id,
                        instance: message_instance,
                        result,
                    },
                )
            }
            MenuAction::Edit => {
                self.game_menu = None;
                self.open_game_edit(&game_id);
                Task::none()
            }
            MenuAction::Metadata => {
                self.game_menu = None;
                self.open_game_metadata(&game_id);
                Task::none()
            }
            MenuAction::ToggleHidden => self.menu_toggle_hidden(&game_id),
            MenuAction::ToggleFavorite => self.menu_toggle_favorite(&game_id),
            MenuAction::Categories => {
                if let Some(menu) = self.game_menu.as_mut() {
                    menu.categories_open = true;
                    menu.notice = None;
                }
                Task::none()
            }
        }
    }

    /// Primeira ação: Jogar (instalado) ou Instalar. Steam instala só abrindo
    /// `steam://install/<appid>`; não passa por `game:launch`.
    fn menu_primary(&mut self, game: Game, instance: u64) -> Task<Message> {
        if game.installed {
            self.game_menu = None;
            return self.request_launch(game.id);
        }

        if !game.launcher.eq_ignore_ascii_case("steam") {
            self.set_menu_notice(
                &game.id,
                MenuNotice::InstallUnavailable(game.launcher.clone()),
            );
            return Task::none();
        }

        let Some(appid) = game.steam_appid().map(str::to_string) else {
            self.set_menu_notice(&game.id, MenuNotice::SteamInvalid);
            return Task::none();
        };
        let backend = self.backend.clone();
        let game_id = game.id.clone();
        // IO do opener numa Task: o teste hermético pode derrubar a Task sem
        // abrir a Steam, e o estado de launch nunca é tocado por esta rota.
        Task::perform(
            async move { backend.open_steam_install(&appid) },
            move |result| Message::GameMenuInstallDone {
                game_id,
                instance,
                result,
            },
        )
    }

    fn menu_toggle_hidden(&mut self, game_id: &str) -> Task<Message> {
        if !self.begin_library_override(game_id) {
            return Task::none();
        }
        if let Some(game) = self.library.game_mut(game_id) {
            game.hidden = !game.hidden;
        }
        self.game_menu = None;
        self.persist_library_override(game_id.to_string())
    }

    fn menu_toggle_favorite(&mut self, game_id: &str) -> Task<Message> {
        if !self.begin_library_override(game_id) {
            return Task::none();
        }
        if let Some(game) = self.library.game_mut(game_id) {
            game.favorite = !game.favorite;
        }
        self.game_menu = None;
        self.persist_library_override(game_id.to_string())
    }

    fn menu_toggle_category(&mut self, category: String) -> Task<Message> {
        let Some(game_id) = self.game_menu.as_ref().map(|menu| menu.game_id.clone()) else {
            return Task::none();
        };
        if !self.begin_library_override(&game_id) {
            return Task::none();
        }
        let mut changed = false;
        if let Some(game) = self.library.game_mut(&game_id) {
            if let Some(index) = game.categories.iter().position(|item| item == &category) {
                game.categories.remove(index);
                changed = true;
            } else if !category.trim().is_empty() {
                game.categories.push(category);
                changed = true;
            }
        }
        if changed {
            self.persist_library_override(game_id)
        } else {
            Task::none()
        }
    }

    fn menu_add_category(&mut self) -> Task<Message> {
        let Some(menu) = self.game_menu.as_mut() else {
            return Task::none();
        };
        let category = menu.category_draft.trim().to_string();
        if category.is_empty() {
            return Task::none();
        }
        let game_id = menu.game_id.clone();
        if !self.begin_library_override(&game_id) {
            return Task::none();
        }
        let mut changed = false;
        if let Some(game) = self.library.game_mut(&game_id) {
            if !game.categories.iter().any(|item| item == &category) {
                game.categories.push(category);
                changed = true;
            }
        }
        if let Some(menu) = self.game_menu.as_mut() {
            menu.category_draft.clear();
        }
        if changed {
            self.persist_library_override(game_id)
        } else {
            Task::none()
        }
    }

    fn begin_library_override(&mut self, game_id: &str) -> bool {
        if !self.library_overrides.busy {
            return true;
        }
        if self.library_overrides.game_id.as_deref() == Some(game_id) {
            return true;
        }
        if self.game_menu.is_some() {
            self.set_menu_notice(game_id, MenuNotice::Busy);
        } else {
            self.library.notice = Some(self.i18n.t("native.library.menu.busy").to_string());
        }
        false
    }

    /// Um patch completo com os três campos que o menu controla: favorito e
    /// oculto como booleanos desejados (nunca `null`, que restauraria um
    /// `true` da base) e categorias sempre como lista (vazia quando o usuário
    /// removeu todas). Campos não editados de overrides não entram no patch.
    fn persist_library_override(&mut self, game_id: String) -> Task<Message> {
        let Some(game) = self.library.game(&game_id) else {
            return Task::none();
        };
        let patch = library_override_patch(game);

        if self.library_overrides.busy {
            if self.library_overrides.game_id.as_deref() == Some(game_id.as_str()) {
                self.library_overrides.dirty = true;
            }
            return Task::none();
        }

        self.library_overrides.busy = true;
        self.library_overrides.game_id = Some(game_id.clone());
        self.library_overrides.dirty = false;
        self.library_overrides.epoch = self.library_overrides.epoch.wrapping_add(1);
        let epoch = self.library_overrides.epoch;
        let account = self.current_account_id();
        self.library_overrides.account = account.clone();
        let backend = self.backend.clone();
        let message_id = game_id.clone();
        let message_account = account.clone();
        Task::perform(
            async move { backend.set_overrides(game_id, patch).await },
            move |result| Message::GameOverridesSaved {
                epoch,
                account: message_account,
                game_id: message_id,
                result,
            },
        )
    }

    fn finish_library_override(
        &mut self,
        epoch: u64,
        account: Option<String>,
        game_id: String,
        result: Result<(), String>,
    ) -> Task<Message> {
        if !self.library_overrides.busy
            || self.library_overrides.epoch != epoch
            || self.library_overrides.game_id.as_deref() != Some(game_id.as_str())
            || account != self.current_account_id()
        {
            return Task::none();
        }
        self.library_overrides.busy = false;
        match result {
            Ok(()) if self.library_overrides.dirty => {
                self.library_overrides.dirty = false;
                self.persist_library_override(game_id)
            }
            Ok(()) => {
                self.library_overrides = LibraryOverrideState::default();
                // A resposta crua de `overrides:set` pode marcar um jogo Steam
                // instalado como falso; a leitura enriquecida acima mantém o
                // reconhecimento da Steam e o epoch da biblioteca.
                self.reload_task()
            }
            Err(error) => {
                self.library_overrides = LibraryOverrideState::default();
                if self.game_menu.as_ref().is_some_and(|menu| menu.game_id == game_id) {
                    self.set_menu_notice(&game_id, MenuNotice::SaveFailed(error));
                } else {
                    self.library.notice = Some(self.i18n.t_args(
                        "native.library.menu.override_failed",
                        &[("error", &error)],
                    ));
                }
                // Relê o estado durável: a atualização otimista não venceu.
                self.reload_task()
            }
        }
    }

    fn open_game_edit(&mut self, id: &str) {
        if let Some(game) = self.library.game(id) {
            self.game_edit = Some(GameEditState::new(game));
        }
    }

    fn open_game_metadata(&mut self, id: &str) {
        if let Some(game) = self.library.game(id) {
            self.game_metadata = Some(MetadataState::new(game));
        }
    }

    fn save_game_edit(&mut self) -> Task<Message> {
        let Some(edit) = self.game_edit.as_mut() else {
            return Task::none();
        };
        if edit.saving {
            return Task::none();
        }
        if let Some(key) = edit.validate() {
            edit.error = Some(self.i18n.t(key).to_string());
            return Task::none();
        }
        let payload = edit.payload();
        if payload.is_empty() {
            self.game_edit = None;
            return Task::none();
        }
        edit.saving = true;
        edit.request = edit.request.wrapping_add(1);
        let request = edit.request;
        let game_id = edit.game_id.clone();
        let account = self.current_account_id();
        let message_account = account.clone();
        let custom = payload.custom;
        let overrides = payload.overrides;
        let backend = self.backend.clone();
        let message_id = game_id.clone();
        let task = Task::perform(
            async move {
                if !custom.is_empty() {
                    backend
                        .update_custom_game(game_id.clone(), custom.title, custom.platform, custom.exe)
                        .await?;
                }
                if overrides.as_object().is_some_and(|map| !map.is_empty()) {
                    backend.set_overrides(game_id.clone(), overrides).await?;
                }
                Ok(())
            },
            move |result| Message::GameEditSaved {
                game_id: message_id.clone(),
                request,
                account: message_account.clone(),
                result,
            },
        );
        task
    }

    fn finish_game_edit(
        &mut self,
        game_id: String,
        request: u64,
        account: Option<String>,
        result: Result<(), String>,
    ) -> Task<Message> {
        let Some(edit) = self.game_edit.as_ref() else {
            return Task::none();
        };
        if edit.game_id != game_id
            || edit.request != request
            || account != self.current_account_id()
        {
            return Task::none();
        }
        match result {
            Ok(()) => {
                self.game_edit = None;
                self.reload_task()
            }
            Err(error) => {
                let message = self.i18n.t_args(
                    "native.library.menu.edit_failed",
                    &[("error", &shorten_error(&error, 160))],
                );
                if let Some(edit) = self.game_edit.as_mut() {
                    edit.saving = false;
                    edit.error = Some(message);
                }
                // A primeira escrita (customgame:update) pode ter vencido e a
                // segunda falhado: relê a biblioteca sem fechar o rascunho.
                self.reload_task()
            }
        }
    }

    fn save_game_metadata(&mut self) -> Task<Message> {
        let Some(metadata) = self.game_metadata.as_mut() else {
            return Task::none();
        };
        if metadata.saving {
            return Task::none();
        }
        if let Some(key) = metadata.validate() {
            metadata.error = Some(self.i18n.t(key).to_string());
            return Task::none();
        }
        let patch = metadata.patch();
        if patch.as_object().is_some_and(|map| map.is_empty()) {
            self.game_metadata = None;
            return Task::none();
        }
        metadata.saving = true;
        metadata.request = metadata.request.wrapping_add(1);
        let request = metadata.request;
        let game_id = metadata.game_id.clone();
        let account = self.current_account_id();
        let message_account = account.clone();
        let backend = self.backend.clone();
        let message_id = game_id.clone();
        Task::perform(
            async move { backend.set_overrides(game_id, patch).await },
            move |result| Message::GameMetadataSaved {
                game_id: message_id,
                request,
                account: message_account,
                result,
            },
        )
    }

    fn finish_game_metadata(
        &mut self,
        game_id: String,
        request: u64,
        account: Option<String>,
        result: Result<(), String>,
    ) -> Task<Message> {
        let Some(metadata) = self.game_metadata.as_ref() else {
            return Task::none();
        };
        if metadata.game_id != game_id
            || metadata.request != request
            || account != self.current_account_id()
        {
            return Task::none();
        }
        match result {
            Ok(()) => {
                self.game_metadata = None;
                self.reload_task()
            }
            Err(error) => {
                let message = self.i18n.t_args(
                    "native.library.menu.metadata_failed",
                    &[("error", &shorten_error(&error, 160))],
                );
                if let Some(metadata) = self.game_metadata.as_mut() {
                    metadata.saving = false;
                    metadata.error = Some(message);
                }
                Task::none()
            }
        }
    }

    /// Abre o diálogo e carrega `gamesettings:get`.
    fn open_launch_options(&mut self, id: String) -> Task<Message> {
        let Some(game) = self.library.game(&id) else {
            return Task::none();
        };
        let title = game.title.clone();
        let default_prefix = self.backend.native_default_prefix(&id);
        // Reabrir a mesma ficha preserva o modo já escolhido, desde que não
        // haja rascunho pendente (nesse caso o modo não seria aplicável).
        let previous_mode = if self.launch_options.game_id == id && !self.launch_options.dirty {
            self.launch_options.mode.clone()
        } else {
            None
        };
        self.launch_options = LaunchOptionsState {
            open: true,
            game_id: id.clone(),
            game_title: title,
            status: LoadStatus::Loading,
            settings: None,
            default_prefix,
            mode: previous_mode,
            advanced_open: false,
            env_name: String::new(),
            env_value: String::new(),
            emulator_args_text: String::new(),
            emulators: Vec::new(),
            emulator_statuses: Vec::new(),
            emulators_error: None,
            saving: false,
            saved: false,
            dirty: false,
            error: None,
            pick_busy: None,
        };

        let backend = self.backend.clone();
        let message_id = id.clone();
        let settings_task = Task::perform(
            async move { backend.load_game_settings(id).await },
            move |result| Message::LaunchOptionsLoaded {
                id: message_id,
                result,
            },
        );

        let backend = self.backend.clone();
        let message_id = self.launch_options.game_id.clone();
        let emulators_task = Task::perform(
            async move { backend.load_game_emulator_options().await },
            move |result| Message::LaunchOptionsEmulatorsLoaded {
                id: message_id,
                result,
            },
        );

        Task::batch([settings_task, emulators_task])
    }

    fn add_launch_env_var(&mut self) {
        let name = self.launch_options.env_name.trim().to_string();
        if name.is_empty() {
            self.launch_options.error =
                Some(self.i18n.t("native.game.env_name_required").to_string());
            return;
        }
        let Some(settings) = self.launch_options.settings.as_mut() else {
            return;
        };
        let value = self.launch_options.env_value.clone();
        settings.env_vars.retain(|variable| variable.name != name);
        settings
            .env_vars
            .push(crate::backend::EnvVarSetting { name, value });
        self.launch_options.env_name.clear();
        self.launch_options.env_value.clear();
        self.launch_options.error = None;
        self.launch_options.mark_dirty();
    }

    /// Salva o patch via `gamesettings:set`; o próximo `game:launch` lê o
    /// arquivo no `RuntimeDeps` do IPC, sem repetir as opções no payload.
    fn save_launch_options(&mut self) -> Task<Message> {
        if self.launch_options.saving {
            return Task::none();
        }
        let Some(settings) = self.launch_options.settings.as_ref() else {
            return Task::none();
        };
        if self.launch_options.mode.as_deref() == Some("exe") && settings.exe_path.trim().is_empty()
        {
            self.launch_options.error =
                Some(self.i18n.t("native.game.exe_required_save").to_string());
            return Task::none();
        }

        let id = self.launch_options.game_id.clone();
        let patch = settings.to_patch();
        let emulator_selected = settings
            .emulator_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty());
        if emulator_selected && settings.rom_path.trim().is_empty() {
            self.launch_options.error =
                Some(self.i18n.t("native.game.emulator_rom_required").to_string());
            return Task::none();
        }

        self.launch_options.saving = true;
        self.launch_options.saved = false;
        // O patch cobre o estado atual; se algo for editado enquanto o save
        // estiver em voo, `mark_dirty` volta a ligar o flag.
        self.launch_options.dirty = false;
        self.launch_options.error = None;

        if emulator_selected {
            // A validação é o mesmo `emulators:resolve` do launch/importação:
            // sem ela o diálogo poderia "salvar" uma ROM/core inválidos.
            let payload = settings.emulator_resolve_payload("hydra");
            let backend = self.backend.clone();
            return Task::perform(
                async move {
                    let resolved = backend.resolve_emulator_launch(payload).await?;
                    if resolved.ok {
                        Ok(())
                    } else {
                        Err(resolved.error_text())
                    }
                },
                move |result| Message::LaunchOptionsValidated { id, patch, result },
            );
        }

        self.save_launch_options_patch(id, patch)
    }

    /// Dispara o `gamesettings:set` de um patch já validado.
    fn save_launch_options_patch(&mut self, id: String, patch: Value) -> Task<Message> {
        let backend = self.backend.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.save_game_settings(id, patch).await },
            move |result| Message::LaunchOptionsSaved {
                id: message_id,
                result,
            },
        )
    }

    /// "Jogar" na ficha ou no Big Picture: dispara `game:launch`.
    ///
    /// O runtime só aceita uma sessão; um segundo jogo é recusado na UI com
    /// mensagem explícita antes de chegar ao backend. `minimize_on_game_launch`
    /// é aplicado só depois do `ok:true`, para uma falha continuar visível.
    fn request_launch(&mut self, id: String) -> Task<Message> {
        if matches!(
            self.launch.status,
            GameRunStatus::Launching | GameRunStatus::Stopping
        ) {
            return Task::none();
        }

        if self.launch.status == GameRunStatus::Running
            && self.launch.game_id.as_deref() == Some(id.as_str())
        {
            return Task::none();
        }

        if self.launch.busy_with_other(&id) {
            let running = self.launch.game_id.clone().unwrap_or_default();
            let title = self
                .library
                .game(&running)
                .map(|game| game.title.clone())
                .unwrap_or(running);
            self.launch.error = Some(
                self.i18n
                    .t_args("native.game.close_running", &[("title", &title)]),
            );
            return Task::none();
        }

        let Some(game) = self.library.game(&id).cloned() else {
            return Task::none();
        };
        if !game.installed {
            self.launch.error = Some(
                self.i18n
                    .t_args("native.game.not_installed_title", &[("title", &game.title)]),
            );
            return Task::none();
        }

        self.launch.status = GameRunStatus::Launching;
        self.launch.game_id = Some(id.clone());
        self.launch.error = None;
        self.launch.warnings.clear();
        self.launch.warnings_game_id = None;
        let mode = self.launch_options.mode_for(&id).map(str::to_string);

        let backend = self.backend.clone();
        Task::perform(
            async move { backend.launch_game(&game, mode.as_deref()).await },
            move |result| Message::LaunchGameDone { id, result },
        )
    }

    /// "Parar": `game:close` encerra o processo e a árvore dele.
    fn request_stop(&mut self) -> Task<Message> {
        if self.launch.status != GameRunStatus::Running {
            return Task::none();
        }

        self.launch.status = GameRunStatus::Stopping;
        self.launch.error = None;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.close_game().await },
            Message::StopGameDone,
        )
    }

    fn finish_launch(&mut self, id: String, result: Result<(), String>) -> Task<Message> {
        // Uma resposta atrasada não pode reabrir uma sessão que já mudou.
        if !self.launch.is_launching(&id) {
            return Task::none();
        }

        match result {
            Ok(()) => {
                eprintln!("[arcadia-native] jogo iniciado: {id}");
                self.launch.status = GameRunStatus::Running;
                self.launch.game_id = Some(id);
                self.launch.error = None;
                if self.launch.minimize_on_launch && self.window.is_some() {
                    self.launch.minimized = true;
                    self.window_task(|window| window::minimize(window, true))
                } else {
                    Task::none()
                }
            }
            Err(error) => {
                eprintln!("[arcadia-native] falha ao abrir jogo: {error}");
                self.launch.status = GameRunStatus::Idle;
                self.launch.game_id = None;
                self.launch.error = Some(
                    self.i18n
                        .t_args("native.game.launch_failed", &[("error", &error)]),
                );
                Task::none()
            }
        }
    }

    fn finish_stop(&mut self, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => {
                eprintln!("[arcadia-native] jogo encerrado");
                self.launch.status = GameRunStatus::Idle;
                self.launch.game_id = None;
                self.launch.error = None;
                self.restore_window_task()
            }
            Err(error) => {
                eprintln!("[arcadia-native] falha ao parar jogo: {error}");
                self.launch.status = GameRunStatus::Running;
                self.launch.error = Some(
                    self.i18n
                        .t_args("native.game.stop_failed", &[("error", &error)]),
                );
                Task::none()
            }
        }
    }

    /// Desfaz só a minimização feita pelo launch; não mexe se o usuário
    /// minimizou a janela por conta própria.
    fn restore_window_task(&mut self) -> Task<Message> {
        if !self.launch.minimized {
            return Task::none();
        }
        self.launch.minimized = false;
        self.window_task(|window| window::minimize(window, false))
    }

    fn clear_update_backdrop(&mut self) {
        self.update.backdrop_generation = self.update.backdrop_generation.wrapping_add(1);
        self.update.backdrop = None;
        // An already queued screenshot must finish before any new dialog is
        // drawn, or it could capture that dialog rather than the clean frame.
        self.update.backdrop_waiting = self.update.dialog.is_some() && self.update.backdrop_capture_requested.is_some();
    }

    fn window_event(&mut self, event: window::Event) -> Task<Message> {
        match event {
            window::Event::Resized(size) => {
                self.clear_update_backdrop();
                self.game_menu = None;
                self.window_size = size;
                if matches!(self.screen, Screen::BigPicture) {
                    self.console_rail_scroll()
                } else {
                    Task::none()
                }
            }
            window::Event::Focused => {
                // Voltou da Steam (ou de qualquer app): a instalação pode ter
                // terminado. Refresh leve, sem spinner.
                Task::batch([self.light_library_refresh(), self.refresh_visible_section()])
            }
            window::Event::Unfocused => {
                // Direção segurada não deve continuar repetindo ao voltar.
                self.big_picture.reset_input();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// ~60 fps: drena os eventos do gamepad e aplica um por um.
    fn console_tick(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        self.big_picture.refresh_clock();
        if self.big_picture.overview_open() && self.media.is_active() {
            self.media.tick();
        }
        let events = self.big_picture.tick(Duration::from_millis(16));
        Task::batch(
            events
                .into_iter()
                .map(|event| self.console_nav(event))
                .collect::<Vec<_>>(),
        )
    }

    fn console_key(&mut self, event: keyboard::Event) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }
        if self.store.detail.as_ref().is_some_and(|detail| detail.downloads.open) {
            return self.update(Message::StoreDownloadKey(event));
        }

        let keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        } = &event
        else {
            return Task::none();
        };

        use keyboard::key::Named;

        if matches!(key, keyboard::Key::Named(Named::F5)) {
            self.library.set_loading();
            return self.reload_task();
        }

        // Tab/Shift+Tab percorrem a ordem de foco; L1/R1 (PageUp/PageDown no
        // teclado) continuam trocando de aba.
        if matches!(key, keyboard::Key::Named(Named::Tab)) {
            return self.console_focus_step(!modifiers.shift());
        }

        if self.big_picture.tab() == ConsoleTab::Stores {
            if self.big_picture.store_editing() {
                return match key {
                    keyboard::Key::Named(Named::Escape) => self.console_nav(NavEvent::Back),
                    keyboard::Key::Named(Named::ArrowDown) => self.console_nav(NavEvent::Down),
                    // Enter/Space são consumidos pelo `text_input` (on_submit
                    // e edição do valor); não duplicar a busca aqui.
                    _ => Task::none(),
                };
            }

            if self.big_picture.zone() == ConsoleZone::StoreSearch && !modifiers.control() {
                if let Some(text) = text {
                    let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !typed.is_empty() && !modifiers.alt() {
                        // O campo ainda não tinha foco: o primeiro caractere é
                        // aplicado direto e o `text_input` recebe o foco para o
                        // restante da digitação.
                        self.store.query.push_str(&typed);
                        self.big_picture.begin_store_editing();
                        let query = self.store.query.clone();
                        let suggestions = self.update(Message::StoreQueryChanged(query));
                        let focus = self.console_store_focus_task(true);
                        return Task::batch([suggestions, focus]);
                    }
                }
            }
        }

        match console_nav_from_key(&event) {
            Some(nav) => self.console_nav(nav),
            None => Task::none(),
        }
    }

    fn console_focus_step(&mut self, forward: bool) -> Task<Message> {
        let was_editing = self.big_picture.store_editing();
        self.big_picture.focus_step(forward);
        let focus = if was_editing != self.big_picture.store_editing() {
            self.console_store_focus_task(self.big_picture.store_editing())
        } else {
            Task::none()
        };
        Task::batch([focus, self.ensure_console_store()])
    }

    fn console_nav(&mut self, event: NavEvent) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }
        if let Some(detail) = self.store.detail.as_ref().filter(|detail| detail.downloads.open) {
            match event {
                NavEvent::Back => return self.update(Message::StoreDownloadClosed),
                NavEvent::Confirm => return self.update(Message::StoreSourceDownloadConfirmed),
                NavEvent::Up | NavEvent::Down if !detail.downloads.data.offers.is_empty() => {
                    let count = detail.downloads.data.offers.len();
                    let index = if event == NavEvent::Up {
                        (detail.downloads.selected_offer + count - 1) % count
                    } else {
                        (detail.downloads.selected_offer + 1) % count
                    };
                    let reference = detail.downloads.data.offers[index].reference.clone();
                    return self.update(Message::StoreSourceSelected(reference));
                }
                NavEvent::Left | NavEvent::Right => {
                    if let Some(offer) = detail.downloads.data.offers.get(detail.downloads.selected_offer) {
                        let count = offer.uris.len();
                        if count > 0 {
                            let index = if event == NavEvent::Left {
                                (detail.downloads.selected_uri + count - 1) % count
                            } else {
                                (detail.downloads.selected_uri + 1) % count
                            };
                            return self.update(Message::StoreSourceUriSelected(index));
                        }
                    }
                }
                _ => {}
            }
            return Task::none();
        }
        if self.big_picture.store_detail_open() && event == NavEvent::Confirm {
            return self.update(Message::StoreDownloadRequested);
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let before = self.big_picture.active_index();
        let before_media = self.big_picture.overview_media();
        let before_store = self.big_picture.store_index();
        let was_editing = self.big_picture.store_editing();
        let effect = self.big_picture.handle(event, &games);
        let moved = before != self.big_picture.active_index();
        let editing_changed = was_editing != self.big_picture.store_editing();
        let scroll = if self.big_picture.tab() == ConsoleTab::Stores && before_store != self.big_picture.store_index() {
            views::big_picture_store::scroll_task(self.big_picture.store_index(), self.window_size.width, false)
        } else if self.big_picture.tab() == ConsoleTab::Stores && matches!(self.big_picture.zone(), ConsoleZone::StoreHero | ConsoleZone::StoreCategories | ConsoleZone::StoreSearch) {
            views::big_picture_store::scroll_task(0, self.window_size.width, true)
        } else if before_media != self.big_picture.overview_media() {
            views::big_picture_detail::gallery_scroll_task(self.big_picture.overview_media(), self.window_size.width)
        } else if moved {
            self.console_rail_scroll()
        } else {
            Task::none()
        };
        let focus = if editing_changed {
            self.console_store_focus_task(self.big_picture.store_editing())
        } else {
            Task::none()
        };
        Task::batch([
            scroll,
            focus,
            self.console_effect(effect),
            self.console_sync_media(),
            self.ensure_console_news(),
            self.ensure_console_store(),
        ])
    }

    fn console_select_game(&mut self, index: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let (moved, effect) = self.big_picture.press_game(index, &games);
        let scroll = if moved {
            self.console_rail_scroll()
        } else {
            Task::none()
        };
        Task::batch([
            scroll,
            self.console_effect(effect),
            self.console_sync_media(),
            self.ensure_console_news(),
        ])
    }

    fn console_select_tab(&mut self, tab: ConsoleTab) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let was_editing = self.big_picture.store_editing();
        self.big_picture.select_tab(tab, &games);
        let focus = if was_editing && !self.big_picture.store_editing() {
            self.console_store_focus_task(false)
        } else {
            Task::none()
        };
        Task::batch([
            focus,
            self.console_rail_scroll(),
            self.console_sync_media(),
            self.ensure_console_news(),
            self.ensure_console_store(),
        ])
    }

    fn console_action(&mut self, index: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let games = console_shelf_for(&self.library, self.big_picture.tab());
        let effect = self.big_picture.press_action(index, &games);
        Task::batch([
            self.console_effect(effect),
            self.console_sync_media(),
            self.ensure_console_news(),
        ])
    }

    fn console_effect(&mut self, effect: ConsoleEffect) -> Task<Message> {
        match effect {
            ConsoleEffect::None => Task::none(),
            ConsoleEffect::Exit => self.exit_big_picture(),
            ConsoleEffect::LaunchGame(id) => {
                if self.launch.is_running(&id) {
                    self.request_stop()
                } else {
                    self.request_launch(id)
                }
            }
            ConsoleEffect::OpenGameDetail(id) => self.open_game_from_console(id),
            ConsoleEffect::ToggleFavorite(id) => self.toggle_console_favorite(id),
            ConsoleEffect::Trailer(id) => self.console_trailer(id),
            ConsoleEffect::OverviewMedia(index) => self.console_open_overview_media(index),
            ConsoleEffect::Notice(notice) => {
                let text = match notice {
                    crate::big_picture::ConsoleNotice::NotInstalled(title) => self
                        .i18n
                        .t_args("native.console.notice.not_installed", &[("title", &title)]),
                    crate::big_picture::ConsoleNotice::NoGameSelected => {
                        self.i18n.t("native.console.notice.no_game").to_string()
                    }
                };
                self.big_picture.set_notice(text);
                Task::none()
            }
            ConsoleEffect::OpenStoreDetail(index) => {
                let game = self.console_store_games().get(index).copied().cloned();
                match game {
                    Some(game) if self.big_picture.open_store_detail() => {
                        self.open_store_detail(game)
                    }
                    _ => Task::none(),
                }
            }
            ConsoleEffect::CloseStoreDetail => {
                self.big_picture.close_store_detail();
                self.store.detail = None;
                Task::none()
            }
            ConsoleEffect::StoreLoadMore => self.load_store_more(),
            ConsoleEffect::StoreHeroStep(delta) => self.update(Message::ConsoleStoreHeroStep(delta)),
            ConsoleEffect::StoreOpenHero => self.update(Message::ConsoleStoreHeroOpen),
            ConsoleEffect::StoreCategory(index) => self.update(Message::ConsoleStoreCategory(index)),
            ConsoleEffect::StoreSearch => {
                let task = self.submit_store_search();
                let sync = self.ensure_console_store();
                Task::batch([task, sync])
            }
        }
    }

    /// Mantém o trailer do painel alinhado ao jogo selecionado. Ao fechar o
    /// painel (ou trocar de aba) o pipeline é descartado; ao abrir/trocar de
    /// jogo, um novo `game:sysinfo` é disparado sem bloquear a UI.
    fn console_sync_media(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) {
            return Task::none();
        }

        let selected = if self.big_picture.overview_open() {
            let games = console_shelf_for(&self.library, self.big_picture.tab());
            self.big_picture
                .selected(&games)
                .map(|game| (game.id.clone(), game.title.clone()))
        } else {
            None
        };

        let Some((id, title)) = selected else {
            self.clear_console_media();
            if self.big_picture.tab() == ConsoleTab::Stores {
                let game = self.console_store_games().get(self.big_picture.store_index()).map(|g| (*g).clone());
                return game.map(|g| self.prefetch_store(g)).unwrap_or_else(Task::none);
            }
            if !matches!(self.big_picture.tab(), ConsoleTab::Home | ConsoleTab::Library) { return Task::none(); }
            let games = console_shelf_for(&self.library, self.big_picture.tab());
            let id = self.big_picture.selected(&games).map(|game| game.id.clone());
            return id.map(|id| self.prefetch_game(id)).unwrap_or_else(Task::none);
        };

        if self.console_media_id.as_deref() == Some(id.as_str()) {
            return Task::none();
        }

        self.media.clear();
        self.game_media = None;
        self.console_media_id = Some(id.clone());
        self.selected_screenshot = 0;
        self.gallery_trailer = true;
        self.big_picture.sync_overview_media(0);
        let panels = match self.library.game(&id).cloned() {
            Some(game) => self.game_panels_task(&game),
            None => Task::none(),
        };
        let cached = self.game_media_cache.get(&id).filter(|(at, _)| at.elapsed() < Duration::from_secs(600)).map(|(_, m)| m.clone());
        // Mesma razão do `open_game`: aplicar o cache aqui abre o trailer
        // dentro do frame de navegação do controle.
        let media = if let Some(media) = cached {
            Task::perform(async move { media }, move |media| Message::ConsoleMediaLoaded { id, media })
        } else {
            let backend = self.backend.clone();
            let request_id = id.clone();
            Task::perform(async move { backend.load_game_media(request_id, title).await }, move |media| Message::ConsoleMediaLoaded { id, media })
        };
        Task::batch([media, panels])
    }

    fn console_open_overview_media(&mut self, index: usize) -> Task<Message> {
        let Some(media) = self.game_media.as_ref() else { return Task::none(); };
        let trailers = usize::from(!media.movies.is_empty());
        if index >= trailers + media.screenshots.len() { return Task::none(); }
        self.big_picture.select_overview_media(index);
        if trailers > 0 && index == 0 {
            self.update(Message::SelectTrailer)
        } else {
            self.update(Message::SelectScreenshot(index - trailers))
        }
    }

    fn clear_console_media(&mut self) {
        if self.console_media_id.take().is_some() {
            self.media.clear();
            self.game_media = None;
        }
    }

    fn ensure_console_news(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture) || self.big_picture.tab() != ConsoleTab::News
        {
            return Task::none();
        }
        if self.news_loaded_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(120)) {
            self.load_console_news()
        } else {
            Task::none()
        }
    }

    fn load_console_news(&mut self) -> Task<Message> {
        if self.console_news.status == LoadStatus::Loading {
            return Task::none();
        }
        self.news_loaded_at = Some(Instant::now());
        self.console_news.status = LoadStatus::Loading;
        self.console_news.error = None;
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_news().await },
            Message::ConsoleNewsLoaded,
        )
    }

    fn toggle_console_favorite(&mut self, id: String) -> Task<Message> {
        if self.favorite_busy.is_some() {
            self.big_picture.set_notice(
                self.i18n
                    .t("native.console.notice.favorite_wait")
                    .to_string(),
            );
            return Task::none();
        }

        let Some(game) = self.library.game(&id).cloned() else {
            self.big_picture.set_notice(
                self.i18n
                    .t("native.console.notice.favorite_missing")
                    .to_string(),
            );
            return Task::none();
        };
        let favorite = !game.favorite;
        self.favorite_busy = Some(id.clone());
        // O painel continua mostrando o estado antigo até o dispatcher
        // responder com a biblioteca atualizada.
        self.big_picture.set_notice(if favorite {
            self.i18n.t_args(
                "native.console.notice.favorite_adding",
                &[("title", &game.title)],
            )
        } else {
            self.i18n.t_args(
                "native.console.notice.favorite_removing",
                &[("title", &game.title)],
            )
        });

        let backend = self.backend.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.set_favorite(id, favorite).await },
            move |result| Message::ConsoleFavoriteDone {
                id: message_id,
                result,
            },
        )
    }

    fn console_trailer(&mut self, id: String) -> Task<Message> {
        self.big_picture.select_overview_tab(crate::big_picture::OverviewTab::Media);
        self.big_picture.select_overview_media(0);
        self.gallery_trailer = true;
        if self.console_media_id.as_deref() != Some(id.as_str()) {
            return self.console_sync_media();
        }

        if self.media.can_control() {
            return match self.media.toggle() {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            };
        }

        let sources = self
            .game_media
            .as_ref()
            .and_then(|media| media.movies.first())
            .map(|movie| media::TrailerSources {
                hls: movie.hls.clone(),
                mp4: movie.mp4.clone(),
                webm: movie.webm.clone(),
            });
        if let Some(sources) = sources {
            return match self.media.open(&sources) {
                Ok(()) => Task::none(),
                Err(error) => Task::done(Message::MediaError(error)),
            };
        }

        if self.game_media.is_some() {
            let notice = self
                .game_media
                .as_ref()
                .and_then(|media| media.notice.clone())
                .unwrap_or_else(|| self.i18n.t("native.game.trailer_unavailable").to_string());
            self.big_picture.set_notice(notice);
            Task::none()
        } else {
            self.big_picture
                .set_notice(self.i18n.t("native.game.trailer_still_loading").to_string());
            Task::none()
        }
    }

    /// Abre a ficha desktop saindo do console sem restaurar o trailer da tela
    /// anterior (o `open_game` já dispara a mídia do jogo escolhido).
    fn open_game_from_console(&mut self, id: String) -> Task<Message> {
        let mut tasks = Vec::new();
        if matches!(self.screen, Screen::BigPicture) {
            self.previous_screen = None;
            self.big_picture.leave();
            self.media.clear();
            self.game_media = None;
            self.console_media_id = None;
            if let Some(window) = self.window {
                tasks.push(window::set_mode(window, window::Mode::Windowed));
            }
        }
        tasks.push(self.open_game(id));
        Task::batch(tasks)
    }

    fn focus_console_game(&mut self, id: &str) {
        if self.big_picture.tab() == ConsoleTab::Stores { return; }
        let games = console_shelf_for(&self.library, self.big_picture.tab());
        if let Some(index) = games.iter().position(|game| game.id == id) {
            self.big_picture.focus_rail(index);
        }
    }

    fn console_rail_scroll(&self) -> Task<Message> {
        views::big_picture::rail_scroll_task(
            self.big_picture.active_index(),
            self.window_size.width,
        )
    }

    /// Timer de ~60 fps que drena os frames do GStreamer e o gamepad do
    /// console, mais o tick de ~150 ms que drena os eventos de lançamento.
    ///
    /// O canal do `NativeShell` é a fonte de verdade da sessão de jogo: não há
    /// mais consulta periódica ao `LaunchRuntime`. O tick de eventos existe em
    /// qualquer tela (inclusive no Big Picture) e não acorda nada além de uma
    /// fila em memória quando está vazia.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();
        let page_animating = !self.reduce_motion()
            && self.page_entered.elapsed() < Duration::from_millis(320);
        let home_animating = !self.boot.active
            && !self.account.auth.open
            && matches!(self.screen, Screen::Section(Section::Home))
            && self.store.detail.is_none()
            && views::home::art_is_animating(self);
        if let Some(interval) = views::home::animation_interval(page_animating || home_animating) {
            subscriptions.push(iced::time::every(interval).map(|_| Message::HomeArtTick));
        }
        if !self.boot.active
            && !self.account.auth.open
            && views::home::carousel_is_active(self)
        {
            subscriptions.push(
                iced::time::every(views::home::carousel_interval()).map(|_| Message::HomeArtTick),
            );
        }

        if self.update.backdrop_waiting && self.update.backdrop_capture_requested.is_none() {
            subscriptions.push(window::frames().map(|_| Message::UpdateBackdropFrame));
        }

        subscriptions.push(window::events().filter_map(|(_, event)| match event {
            window::Event::Resized(_) | window::Event::Focused | window::Event::Unfocused => {
                Some(Message::WindowEvent(event))
            }
            _ => None,
        }));

        // Cards animados do `about` rodam em loop como na página da Steam:
        // precisam de um tick de 16 ms enquanto houver pipeline ativo.
        if self.animated_cards.is_active() || (matches!(self.screen,Screen::Section(Section::Friends)) && self.friend_media.is_active()) {
            subscriptions.push(
                iced::time::every(Duration::from_millis(16)).map(|_| Message::CardsTick),
            );
        }

        if self.boot.active {
            subscriptions
                .push(iced::time::every(Duration::from_millis(16)).map(|_| Message::BootVideoTick));
            subscriptions.push(iced::keyboard::listen().map(Message::BootVideoKey));
        }

        if matches!(self.screen, Screen::BigPicture) {
            subscriptions
                .push(iced::time::every(Duration::from_millis(16)).map(|_| Message::ConsoleTick));
            subscriptions.push(iced::keyboard::listen().map(Message::ConsoleKey));
        } else if (matches!(self.screen, Screen::Game(_)) || self.store.detail.is_some())
            && self.media.is_active()
        {
            subscriptions
                .push(iced::time::every(Duration::from_millis(16)).map(|_| Message::MediaTick));
        }

        if self
            .update
            .dialog
            .as_ref()
            .is_some_and(|dialog| dialog.is_busy() && dialog.kind == UpdateDialogKind::Packaged)
        {
            subscriptions.push(
                iced::time::every(Duration::from_millis(500)).map(|_| Message::UpdatePollTick),
            );
        }

        subscriptions
            .push(iced::time::every(Duration::from_millis(150)).map(|_| Message::LaunchEventsTick));

        if matches!(self.screen, Screen::Section(Section::Stores | Section::Sources | Section::Components))
            || (matches!(self.screen, Screen::BigPicture) && self.big_picture.tab() == ConsoleTab::News)
        {
            subscriptions.push(iced::time::every(Duration::from_secs(60)).map(|_| Message::RefreshVisibleSection));
        }

        if !self.toasts.is_empty() {
            subscriptions
                .push(iced::time::every(Duration::from_millis(50)).map(|_| Message::ToastTick));
        }

        subscriptions
            .push(iced::time::every(Duration::from_secs(15)).map(|_| Message::AchievementsPoll));

        // Só com ficha Steam aberta ainda não instalada: 5 s é barato e evita
        // que o usuário tenha que apertar F5 depois de instalar pela Steam.
        if self.steam_install_pending() {
            subscriptions
                .push(iced::time::every(Duration::from_secs(5)).map(|_| Message::SteamLibraryPoll));
        }

        if self.account.auth.open && !matches!(self.screen, Screen::BigPicture) {
            subscriptions.push(iced::keyboard::listen().map(Message::AccountKey));
        }

        if !matches!(self.screen, Screen::BigPicture)
            && (self.game_menu.is_some()
                || self.game_edit.is_some()
                || self.game_metadata.is_some())
        {
            subscriptions.push(iced::keyboard::listen().map(Message::GameMenuKey));
        }

        // Esc no editor de perfil volta para a página do perfil, como o
        // `useGamepadNav` da web. Fica ligado só enquanto o editor existe.
        if self.account.editor.editing && !matches!(self.screen, Screen::BigPicture) {
            subscriptions.push(iced::keyboard::listen().map(Message::ProfileEditKey));
        }
        if !matches!(self.screen, Screen::BigPicture)
            && self.store.detail.as_ref().is_some_and(|detail| detail.downloads.open)
        {
            subscriptions.push(iced::keyboard::listen().map(Message::StoreDownloadKey));
        }

        Subscription::batch(subscriptions)
    }

    pub fn reduce_motion(&self) -> bool {
        std::env::var("ARCADIA_REDUCE_MOTION").as_deref() == Ok("1")
            || self.settings.config.as_ref().map_or(self.launch_reduce_motion, |config| config.no_anim)
    }

    pub fn view(&self) -> Element<'_, Message> {
        if matches!(self.screen, Screen::BigPicture) {
            return views::big_picture::view(self);
        }
        if self.account.auth.open {
            return views::auth::view(self);
        }

        let base: Element<'_, Message> = if matches!(self.screen, Screen::BigPicture) {
            views::big_picture::view(self)
        } else {
            let sidebar = views::sidebar::view(self, &self.animated_cards);
            let content: Element<'_, Message> = match &self.screen {
                Screen::Library => views::library::view(self),
                Screen::Game(id) => match self.library.game(id) {
                    Some(game) => {
                        let community = game
                            .steam_appid()
                            .and_then(|appid| self.community.get(appid));
                        views::game::view(
                            game,
                            &self.covers,
                            &self.media,
                            &self.animated_cards,
                            self.game_media.as_ref(),
                            self.game_panels.get(id),
                            community,
                            self.selected_screenshot,
                            self.gallery_trailer,
                            self.requirements_tab,
                            &self.launch,
                            &self.trailer_picker,
                            &self.launch_options,
                            &self.i18n,
                        )
                    }
                    None => views::missing_game(&self.i18n),
                },
                Screen::Section(Section::Downloads) => views::downloads::view(self),
                Screen::Section(Section::Settings) => views::settings::view(self),
                Screen::Section(Section::Sources) => views::sources::view(self),
                Screen::Section(Section::Friends) => views::friends::view(self),
                Screen::Section(Section::Components) => views::components::view(self),
                Screen::Section(Section::Home) => views::home::view(self),
                Screen::Section(Section::Stores) => views::store::view(self),
                Screen::Section(section) => views::section_placeholder(*section, &self.i18n),
                Screen::Profile => {
                    views::profile::view(self, &self.animated_cards)
                }
                Screen::BigPicture => views::big_picture::view(self),
            };

            let progress = if self.reduce_motion() { 1.0 } else {
                let t = (self.page_entered.elapsed().as_secs_f32() / 0.32).clamp(0.0, 1.0);
                1.0 - (1.0 - t).powi(3)
            };
            let offset = 14.0 * (1.0 - progress);
            let content = iced::widget::container(content).width(Fill).height(Fill)
                .padding(iced::Padding {
                    top: 6.0 * (1.0 - progress), bottom: 0.0,
                    left: if self.page_direction > 0.0 { offset } else { 0.0 },
                    right: if self.page_direction < 0.0 { offset } else { 0.0 },
                });
            let content = iced::widget::Stack::new().width(Fill).height(Fill)
                .push(content)
                .push(iced::widget::container(iced::widget::Space::new()).width(Fill).height(Fill)
                    .style(move |_| iced::widget::container::Style {
                        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 1.0 - progress).into()),
                        ..Default::default()
                    }));

            iced::widget::column![
                views::chrome::top_bar(),
                iced::widget::row![
                    sidebar,
                    iced::widget::container(content)
                        .width(Fill)
                        .height(Fill)
                        .style(theme::app_background)
                ]
                .width(Fill)
                .height(Fill)
            ]
            .width(Fill)
            .height(Fill)
            .into()
        };

        views::overlays::stack(self, base)
    }

    /// Carrega catálogo, status, perfis e índice sem bloquear a UI. `detect`
    /// reaproveita a Subscription de tasks e força `emulators:detect`.
    fn load_emulation(&mut self, detect: bool) -> Task<Message> {
        if self.emulation.status == LoadStatus::Loading {
            return Task::none();
        }
        self.emulation.status = LoadStatus::Loading;
        self.emulation.warnings.clear();
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_emulation(detect).await },
            Message::EmulationLoaded,
        )
    }

    /// Abre o editor com o perfil persistido (ou o executável detectado) e a
    /// prévia do índice de ROMs já salvo.
    fn open_emulation_editor(&mut self, id: &str) {
        let Some(item) = self.emulation.emulators.iter().find(|item| item.id == id) else {
            return;
        };
        let profile = item.profile.clone();
        let detected_args = item.detected_args.clone();
        let executable = match profile.as_ref() {
            Some(profile) if !profile.executable.trim().is_empty() => profile.executable.clone(),
            _ => item.executable.clone(),
        };
        let args_text = match profile.as_ref() {
            Some(profile) => profile.args.join(" "),
            None => detected_args.join(" "),
        };
        let requires_core = item.requires_core || id == "retroarch";
        let bios_required = self
            .emulation
            .status_for(id)
            .map(|status| status.required)
            .unwrap_or(false);
        let preview = self.emulation.rom_index_for(id);
        let preview_available = preview.is_some();
        let preview_roms = preview.map(|entry| entry.roms.clone()).unwrap_or_default();
        let preview_truncated = preview.map(|entry| entry.truncated).unwrap_or(false);
        let (core_path, bios_path, rom_folders) = match profile.as_ref() {
            Some(profile) => (
                profile.core_path.clone(),
                profile.bios_path.clone(),
                profile.rom_folders.clone(),
            ),
            None => (String::new(), String::new(), Vec::new()),
        };

        self.emulation.editor = Some(EmulationEditorState {
            emulator_id: id.to_string(),
            executable,
            core_path,
            bios_path,
            args_text,
            rom_folders,
            folder_draft: String::new(),
            original: profile,
            requires_core,
            bios_required,
            saving: false,
            confirm_remove: false,
            error: None,
            scanning: false,
            scan_error: None,
            preview_available,
            preview_roms,
            preview_truncated,
            importing_rom: None,
            notice: None,
            pick_busy: None,
        });
    }

    /// Depois de listar/detectar, o perfil e o status podem ter mudado. O
    /// rascunho do usuário fica intacto; só os campos derivados são renovados.
    fn refresh_emulation_editor(&mut self) {
        let Some(editor) = self.emulation.editor.as_ref() else {
            return;
        };
        let id = editor.emulator_id.clone();
        let Some(item) = self.emulation.emulators.iter().find(|item| item.id == id) else {
            self.emulation.editor = None;
            return;
        };
        let profile = item.profile.clone();
        let requires_core = item.requires_core || id == "retroarch";
        let bios_required = self
            .emulation
            .status_for(&id)
            .map(|status| status.required)
            .unwrap_or(false);
        if let Some(editor) = self.emulation.editor.as_mut() {
            editor.original = profile;
            editor.requires_core = requires_core;
            editor.bios_required = bios_required;
        }
    }

    fn refresh_visible_section(&mut self) -> Task<Message> {
        let section = match self.screen {
            Screen::BigPicture => return self.ensure_console_news(),
            Screen::Section(section) => section,
            _ => return Task::none(),
        };
        let retro = section == Section::Stores && self.store.tab == StoreTab::Retro;
        let status = match section {
            Section::Home if self.store.detail.is_none() => &self.home.status,
            Section::Sources if !self.sources.busy => &self.sources.status,
            Section::Components if self.components.busy.is_none() && !self.components.registering => {
                &self.components.status
            }
            Section::Stores if self.store.detail.is_none() && self.retro.detail.is_none() => {
                if retro {
                    &self.retro.status
                } else if self.store.search.is_none()
                    && !self.store.browse.loading_more
                    && self.store.browse.games.len() <= STORE_PAGE_SIZE as usize
                {
                    &self.store.browse.status
                } else {
                    return Task::none();
                }
            }
            _ => return Task::none(),
        };
        let key = (section, retro);
        if *status == LoadStatus::Loading
            || self.section_loaded_at.get(&key).is_some_and(|at| at.elapsed() < Duration::from_secs(120))
        {
            return Task::none();
        }
        self.section_loaded_at.insert(key, Instant::now());
        if retro {
            let has_page = self.retro.status == LoadStatus::Ready;
            let task = self.load_retro_page(self.retro.offset);
            if has_page {
                self.retro.showing_cached_page = true;
                self.retro.status = LoadStatus::Ready;
            }
            task
        } else {
            self.load_section(section, true)
        }
    }

    /// Carrega a seção sob demanda e permite novas tentativas após erro.
    fn load_section(&mut self, section: Section, force: bool) -> Task<Message> {
        match section {
            Section::Home if force || matches!(self.home.status, LoadStatus::Idle) => {
                self.section_loaded_at.insert((Section::Home, false), Instant::now());
                self.home.status = LoadStatus::Loading;
                self.home.error = None;
                super::timing::mark("home");
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_home_recent(24).await },
                    Message::HomeLoaded,
                )
            }
            Section::Downloads => {
                if self.downloads.status != LoadStatus::Ready {
                    self.downloads.status = LoadStatus::Loading;
                }
                self.downloads.action_error = None;
                self.downloads_task()
            }
            Section::Settings if force || matches!(self.settings.status, LoadStatus::Idle) => {
                self.settings.status = LoadStatus::Loading;
                self.settings.save_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_settings().await },
                    Message::SettingsLoaded,
                )
            }
            Section::Sources if force || matches!(self.sources.status, LoadStatus::Idle) => {
                self.sources.status = LoadStatus::Loading;
                self.sources.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_sources().await },
                    Message::SourcesLoaded,
                )
            }
            Section::Friends if force || matches!(self.friends.status, LoadStatus::Idle) => {
                if !self.account.is_signed_in() {
                    self.friends.data = Some(FriendsLoad::NotLoggedIn);
                    self.friends.status = LoadStatus::Ready;
                    return Task::none();
                }
                self.friends.status = LoadStatus::Loading;
                self.friends.action_error = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_friends(force).await },
                    Message::FriendsLoaded,
                )
            }
            Section::Components if force || matches!(self.components.status, LoadStatus::Idle) => {
                self.components.status = LoadStatus::Loading;
                self.components.banner = None;
                let backend = self.backend.clone();
                Task::perform(
                    async move { backend.load_plugins().await },
                    Message::ComponentsLoaded,
                )
            }
            Section::Stores => match self.store.tab {
                StoreTab::Steam => {
                    if force && self.store.search.is_some() {
                        self.submit_store_search()
                    } else {
                        let should_load =
                            force || matches!(self.store.browse.status, LoadStatus::Idle);
                        if should_load {
                            self.load_store_browse(force)
                        } else {
                            Task::none()
                        }
                    }
                }
                StoreTab::Retro => {
                    let should_load = force || matches!(self.retro.status, LoadStatus::Idle | LoadStatus::Error(_));
                    if should_load {
                        self.load_retro_page(0)
                    } else {
                        self.retro_page_cover_tasks(RETRO_PAGE_SIZE as usize)
                    }
                }
            },
            _ => Task::none(),
        }
    }

    fn downloads_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        let revisions = self.downloads.revisions;
        Task::perform(
            async move { backend.load_downloads().await },
            move |result| Message::DownloadsLoaded { revisions, result },
        )
    }

    /// Dispara a busca com o texto atual; chamado pelo Enter, pelo botão e
    /// pelo clique numa sugestão.
    fn submit_store_search(&mut self) -> Task<Message> {
        let query = self.store.query.trim().to_string();
        if query.is_empty() {
            return self.update(Message::StoreClearSearch);
        }
        self.store_thumbnail_queue.clear();
        self.store.suggestions.clear();
        self.store.search = Some(StoreSearchState {
            query: query.clone(),
            status: LoadStatus::Loading,
            result: None,
            error: None,
        });
        self.sync_console_store();
        let backend = self.backend.clone();
        let message_query = query.clone();
        Task::perform(
            async move { backend.search_store(query).await },
            move |result| Message::StoreSearchDone {
                query: message_query,
                result,
            },
        )
    }

    fn load_store_browse(&mut self, force: bool) -> Task<Message> {
        if !force && self.store.browse.status == LoadStatus::Loading {
            return Task::none();
        }
        self.section_loaded_at.insert((Section::Stores, false), Instant::now());
        self.store.browse.generation = self.store.browse.generation.wrapping_add(1);
        self.store.browse.next_page = None;
        self.store.browse.status = LoadStatus::Loading;
        self.store.browse.error = None;
        self.store.browse.loading_more = false;
        self.store_page_task(0)
    }

    fn store_page_task(&self, offset: u64) -> Task<Message> {
        let backend = self.backend.clone();
        let generation = self.store.browse.generation;
        Task::perform(
            async move { backend.load_store_recent(offset, STORE_PAGE_SIZE).await },
            move |result| Message::StoreBrowseLoaded {
                offset,
                generation,
                result,
            },
        )
    }

    fn load_store_more(&mut self) -> Task<Message> {
        self.request_next_store_page(true)
    }

    fn request_next_store_page(&mut self, append: bool) -> Task<Message> {
        let browse = &self.store.browse;
        if browse.status != LoadStatus::Ready || browse.loading_more || !browse.has_more {
            return Task::none();
        }
        let offset = browse.next_offset.unwrap_or(browse.games.len() as u64);
        if offset == 0 {
            return Task::none();
        }
        self.store.browse.loading_more = append;
        match self.store.browse.next_page.take() {
            Some(NextStorePage::Ready(page)) if append => self.update(Message::StoreBrowseLoaded {
                offset,
                generation: self.store.browse.generation,
                result: Ok(page),
            }),
            Some(state) => {
                self.store.browse.next_page = Some(state);
                Task::none()
            }
            None => {
                self.store.browse.next_page = Some(NextStorePage::Loading);
                self.store_page_task(offset)
            }
        }
    }

    fn register_description_videos(&mut self, about: Option<&str>) {
        let Some(about) = about else {
            return;
        };
        for block in crate::rich_text::parse_description(about) {
            if let Some(poster) = block.image {
                if !block.video.is_empty() {
                    self.animated_cards.open_video(
                        &block.video,
                        crate::animated_cards::MediaGroup::Card,
                    );
                    self.card_videos.insert(poster, block.video);
                }
            }
        }
    }

    fn open_store_detail(&mut self, game: StoreGame) -> Task<Message> {
        self.store_thumbnail_queue.clear();
        self.animated_cards.clear_cards();
        self.card_videos.clear();
        let mut urls = game.image_urls();
        // Todos os candidatos de portrait, não só o caminho por convenção: o
        // primeiro que existir serve, e baixar os dois em paralelo evita a
        // espera em série quando o preferido dá 404.
        for portrait in crate::backend::store_portrait_candidates(&game) {
            if !urls.contains(&portrait) {
                urls.push(portrait);
            }
        }
        let info_task = self.store_info_task(&game);
        let install_task = self.store_install_info_task(&game);
        let libraries_task = self.store_libraries_task();
        let community_task = self.community_tasks(&game.appid);
        let panels_task = self.store_panels_task(&game);
        self.media.clear();
        self.game_media = None;
        self.selected_screenshot = 0;
        self.gallery_trailer = false;
        self.requirements_tab = RequirementsTab::default();
        self.reset_trailer_state();
        self.retro.detail = None;
        let appid = game.appid.clone();
        let cached = self.store_info_cache.get(&appid).filter(|(at, _)| at.elapsed() < Duration::from_secs(600)).map(|(_, info)| info.clone());
        self.store.detail = Some(StoreDetailState {
            game,
            info: None,
            status: LoadStatus::Loading,
            error: None,
            install: None,
            install_status: LoadStatus::Loading,
            install_error: None,
            libraries: Vec::new(),
            libraries_status: LoadStatus::Loading,
            libraries_error: None,
            selected_library: 0,
            selected_depots: BTreeSet::new(),
            install_busy: false,
            install_message: None,
            selected_screenshot: 0,
            downloads: StoreDownloadsState {
                path: crate::backend::DefaultPaths::resolve(self.settings.config.as_ref().unwrap_or(&SettingsConfig::default())).install,
                ..StoreDownloadsState::default()
            },
        });
        // A ficha abre já com o texto em cache, mas o trailer só começa no
        // próximo ciclo: `StoreDetailLoaded` monta o pipeline do GStreamer.
        let info_task = match cached {
            Some(info) => Task::perform(
                async move { info },
                move |info| Message::StoreDetailLoaded { appid, result: Ok(info) },
            ),
            None => info_task,
        };
        let sources_task = self.store_source_offers_task();
        Task::batch([
            self.cover_tasks(urls),
            info_task,
            install_task,
            libraries_task,
            community_task,
            panels_task,
            sources_task,
            views::game_detail::scroll_to_top(),
        ])
    }

    fn store_source_offers_task(&mut self) -> Task<Message> {
        let Some(detail) = self.store.detail.as_mut() else { return Task::none() };
        if detail.install_busy || detail.downloads.pick_busy { return Task::none() }
        self.store.detail_generation = self.store.detail_generation.wrapping_add(1);
        let generation = self.store.detail_generation;
        detail.downloads.generation = generation;
        detail.downloads.status = LoadStatus::Loading;
        detail.downloads.error = None;
        let game = detail.game.clone();
        Task::batch([true, false].into_iter().map(|cached_only| {
            let appid = game.appid.clone();
            let title = game.title.clone();
            let backend = self.backend.clone();
            Task::perform(
                async move { backend.load_source_offers(title, cached_only).await },
                move |result| Message::StoreSourceOffersLoaded { appid, generation, cached_only, result },
            )
        }))
    }

    /// Itens que a Loja do console desenha: resultado da busca quando existe,
    /// senão o catálogo `store:recent` já acumulado.
    fn console_store_games(&self) -> Vec<&StoreGame> {
        if matches!(self.screen, Screen::BigPicture) { return views::big_picture_store::games(self); }
        if let Some(search) = self.store.search.as_ref() {
            search
                .result
                .as_ref()
                .map(|result| result.games.iter().collect())
                .unwrap_or_default()
        } else {
            self.store.browse.games.iter().collect()
        }
    }

    /// Mantém a grade/foco do console alinhados ao estado real da loja.
    fn sync_console_store(&mut self) {
        let count = self.console_store_games().len();
        let has_more = self.store.search.is_none() && self.store.browse.has_more
            && (!matches!(self.screen, Screen::BigPicture) || self.console_storefront.catalog);
        self.big_picture.sync_store(count, has_more);
    }

    /// Garante o catálogo Steam quando a aba Loja do console está ativa. A
    /// aba Retro do desktop não é portada para o Big Picture nesta onda.
    fn ensure_console_store(&mut self) -> Task<Message> {
        if !matches!(self.screen, Screen::BigPicture)
            || self.big_picture.tab() != ConsoleTab::Stores
        {
            return Task::none();
        }

        if self.store.tab != StoreTab::Steam {
            self.store.tab = StoreTab::Steam;
        }
        self.sync_console_store();

        let language = self.i18n.lang();
        let storefront = if matches!(self.console_storefront.status, LoadStatus::Idle) || self.console_storefront.language != Some(language) || (matches!(self.console_storefront.status, LoadStatus::Ready) && self.console_storefront.loaded_at.is_some_and(|at|at.elapsed()>Duration::from_secs(600))) {
            if matches!(self.console_storefront.status, LoadStatus::Idle) && !self.big_picture.store_editing() && self.store.search.is_none() { self.big_picture.focus_store_hero(); }
            self.console_storefront.status = LoadStatus::Loading;
            self.console_storefront.language = Some(language);
            let backend = self.backend.clone();
            Task::perform(async move { backend.load_console_storefront(language.code().to_string()).await },move |result| Message::ConsoleStorefrontLoaded { language, result })
        } else {Task::none()};
        let home = if matches!(self.home.status, LoadStatus::Idle) { self.load_section(Section::Home, false) } else { Task::none() };
        let art: Vec<_> = views::big_picture_store::hero_games(self).iter()
            .flat_map(|game| [crate::backend::store_hero_url(game), crate::backend::steam_logo_url(&game.appid)])
            .flatten().collect();
        let covers = self.cover_tasks(art);
        let catalog = if self.store.search.is_none() && matches!(self.store.browse.status, LoadStatus::Idle) {
            self.load_store_browse(false)
        } else {
            Task::none()
        };
        Task::batch([catalog, home, covers, storefront])
    }

    /// Foco programático no campo de busca; `false` devolve o foco ao painel.
    fn console_store_focus_task(&self, focused: bool) -> Task<Message> {
        let id = iced::widget::Id::new(views::big_picture::STORE_SEARCH_ID);
        if focused {
            iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(id))
        } else {
            iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
        }
    }

    fn load_retro_page(&mut self, offset: u64) -> Task<Message> {
        self.section_loaded_at.insert((Section::Stores, true), Instant::now());
        self.retro.status = LoadStatus::Loading;
        self.retro.error = None;
        self.retro.showing_cached_page = false;
        self.retro.generation = self.retro.generation.wrapping_add(1);
        let generation = self.retro.generation;
        let payload = Backend::retro_page_params(
            &self.retro.applied_query, self.retro.system.id, offset, RETRO_PAGE_SIZE,
        );
        let cached_payload = payload.clone();
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.cached_retro_page(cached_payload).await },
            move |page| Message::RetroPageCached {
                generation,
                offset,
                payload: payload.clone(),
                page,
            },
        )
    }

    fn apply_retro_page(&mut self, page: RetroPage) -> Task<Message> {
        let changed_page = self.retro.offset != page.offset;
        self.retro.offset = page.offset;
        self.retro.games = page.games;
        self.retro.sources = page.sources;
        self.retro.total = page.total;
        self.retro.total_offers = page.total_offers;
        self.retro.has_more = page.has_more;
        self.retro.status = LoadStatus::Ready;
        self.retro.error = None;
        let limit = if matches!(self.screen, Screen::Section(Section::Stores))
            && self.store.tab == StoreTab::Retro
        {
            RETRO_PAGE_SIZE as usize
        } else {
            8
        };
        let covers = self.retro_page_cover_tasks(limit);
        if changed_page && matches!(self.screen, Screen::Section(Section::Stores))
            && self.store.tab == StoreTab::Retro && self.retro.detail.is_none()
        {
            Task::batch([views::retro::scroll_to_top(), covers])
        } else {
            covers
        }
    }

    fn retro_page_cover_tasks(&mut self, limit: usize) -> Task<Message> {
        let art: Vec<_> = self.retro.games.iter().take(limit).filter_map(RetroGame::card_art).collect();
        self.retro_cover_tasks(art)
    }

    fn submit_retro_search(&mut self) -> Task<Message> {
        self.retro.applied_query = self.retro.query.trim().to_string();
        self.load_retro_page(0)
    }

    fn store_info_task(&self, game: &StoreGame) -> Task<Message> {
        let backend = self.backend.clone();
        let appid = game.appid.clone();
        let message_appid = appid.clone();
        let title = game.title.clone();
        Task::perform(
            async move { backend.load_store_info(appid, title).await },
            move |result| Message::StoreDetailLoaded {
                appid: message_appid,
                result,
            },
        )
    }

    fn home_hero_info_task(&mut self, game: &StoreGame) -> Task<Message> {
        self.home.hero_appid = Some(game.appid.clone());
        if let Some(info) = self.store_info_cache.get(&game.appid).filter(|(at, _)| at.elapsed() < Duration::from_secs(600)).map(|(_, info)| info.clone()) {
            return self.update(Message::HomeHeroInfoLoaded { appid: game.appid.clone(), result: Ok(info) });
        }
        self.home.hero_info = None;
        self.home.hero_status = LoadStatus::Loading;
        let backend = self.backend.clone();
        let appid = game.appid.clone();
        let message_appid = appid.clone();
        let title = game.title.clone();
        Task::perform(
            async move { backend.load_store_info(appid, title).await },
            move |result| Message::HomeHeroInfoLoaded {
                appid: message_appid,
                result,
            },
        )
    }

    fn store_install_info_task(&self, game: &StoreGame) -> Task<Message> {
        let backend = self.backend.clone();
        let appid = game.appid.clone();
        let message_appid = appid.clone();
        Task::perform(
            async move { backend.load_store_install_info(appid).await },
            move |result| Message::StoreInstallInfoLoaded {
                appid: message_appid,
                result,
            },
        )
    }

    fn store_libraries_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_store_libraries().await },
            Message::StoreLibrariesLoaded,
        )
    }

    fn retro_game_task(&self, game: &RetroGame) -> Task<Message> {
        let backend = self.backend.clone();
        let id = game.id.clone();
        let message_id = id.clone();
        Task::perform(
            async move { backend.load_retro_game(id).await },
            move |result| Message::RetroDetailLoaded {
                id: message_id,
                result,
            },
        )
    }

    /// Envelopa um patch `config:set` em uma única chave e recarrega o estado
    /// tipado com a resposta redigida do dispatcher.
    fn save_setting(&mut self, key: &'static str, value: Value) -> Task<Message> {
        self.save_dynamic_setting(key, value)
    }

    /// Igual a [`Self::save_setting`], mas aceita chave emprestada — os tokens
    /// de debrid vêm de `DebridProvider::config_key` em tempo de execução.
    fn save_dynamic_setting(&mut self, key: &'static str, value: Value) -> Task<Message> {
        if self.settings.saving.is_some() {
            return Task::none();
        }
        self.settings.saving = Some(key.to_string());
        self.settings.save_error = None;
        let backend = self.backend.clone();
        Task::perform(
            async move {
                let mut patch = serde_json::Map::new();
                patch.insert(key.to_string(), value);
                backend.save_config(Value::Object(patch)).await
            },
            move |result| Message::SettingsSaved {
                key: key.to_string(),
                result,
            },
        )
    }

    fn window_task<F>(&self, action: F) -> Task<Message>
    where
        F: FnOnce(window::Id) -> Task<Message>,
    {
        self.window.map(action).unwrap_or_else(Task::none)
    }

    /// Recarrega `plugins:list` sem trocar a tela para "Carregando"; a lista
    /// atual continua visível até o resultado chegar.
    fn reload_components(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_plugins().await },
            Message::ComponentsLoaded,
        )
    }

    /// Estado de comunidade do jogo/loja atualmente aberto, já sem `steam:`.
    fn current_community_appid(&self) -> Option<String> {
        match &self.screen {
            Screen::Game(id) => self
                .library
                .game(id)
                .and_then(|game| game.steam_appid())
                .map(crate::backend::community_appid),
            Screen::Section(Section::Stores) => self
                .store
                .detail
                .as_ref()
                .map(|detail| crate::backend::community_appid(&detail.game.appid)),
            _ => None,
        }
    }

    /// Primeira carga do painel para um appid; o cache evita repetir o IPC ao
    /// reabrir a mesma ficha na sessão.
    fn community_tasks(&mut self, appid: &str) -> Task<Message> {
        let appid = crate::backend::community_appid(appid);
        if appid.is_empty() {
            return Task::none();
        }
        let (state, _) = self.community.ensure(&appid);
        if state.reviews.status == LoadStatus::Loading
            || state.collections.status == LoadStatus::Loading
            || state.reviews.loading_more
            || state.collections.loading_more
            || state.last_open_load.is_some_and(|at| at.elapsed() < Duration::from_secs(120))
        {
            return Task::none();
        }
        state.last_open_load = Some(Instant::now());
        let reviews = self.load_community_reviews(&appid, 0, false);
        let collections = self.load_community_collections(&appid, 0, false);
        Task::batch([reviews, collections])
    }

    fn load_community_reviews(&mut self, appid: &str, offset: u64, append: bool) -> Task<Message> {
        let Some(state) = self.community.get_mut(appid) else {
            return Task::none();
        };
        if append {
            if state.reviews.loading_more || !state.reviews.has_more {
                return Task::none();
            }
            state.reviews.loading_more = true;
            state.reviews.error = None;
        } else {
            state.reviews.status = LoadStatus::Loading;
            state.reviews.error = None;
            state.reviews.offline = false;
            state.reviews.has_more = false;
            state.reviews.next_offset = offset;
            state.reviews.total = None;
            state.reviews.loading_more = false;
        }
        let backend = self.backend.clone();
        let request_appid = appid.to_string();
        let message_appid = appid.to_string();
        Task::perform(
            async move {
                backend
                    .load_community_reviews(
                        request_appid,
                        offset,
                        views::community::COMMUNITY_PAGE_SIZE,
                    )
                    .await
            },
            move |result| Message::CommunityReviewsLoaded {
                appid: message_appid,
                offset,
                append,
                result,
            },
        )
    }

    fn load_community_collections(
        &mut self,
        appid: &str,
        offset: u64,
        append: bool,
    ) -> Task<Message> {
        let Some(state) = self.community.get_mut(appid) else {
            return Task::none();
        };
        let mine = state.mine_only;
        if append {
            if state.collections.loading_more || !state.collections.has_more {
                return Task::none();
            }
            state.collections.loading_more = true;
            state.collections.error = None;
        } else {
            state.collections.status = LoadStatus::Loading;
            state.collections.error = None;
            state.collections.offline = false;
            state.collections.has_more = false;
            state.collections.next_offset = offset;
            state.collections.total = None;
            state.collections.loading_more = false;
        }
        let backend = self.backend.clone();
        let message_appid = appid.to_string();
        Task::perform(
            async move {
                backend
                    .load_community_collections(offset, views::community::COMMUNITY_PAGE_SIZE, mine)
                    .await
            },
            move |result| Message::CommunityCollectionsLoaded {
                appid: message_appid,
                offset,
                append,
                result,
            },
        )
    }

    /// Estado de comunidade do appid aberto; `None` fora de ficha/detalhe.
    fn current_community_state_mut(&mut self) -> Option<&mut CommunityState> {
        let appid = self.current_community_appid()?;
        self.community.get_mut(&appid)
    }

    /// Mantém o painel ciente da sessão mesmo quando o cache já existia.
    /// Se a conta mudou, o cache foi descartado e a ficha aberta recarrega.
    fn sync_community_viewer(&mut self) -> Task<Message> {
        let viewer = self
            .account
            .session
            .as_ref()
            .map(|session| session.id.clone());
        if !self.community.set_viewer(viewer) {
            return Task::none();
        }
        match self.current_community_appid() {
            Some(appid) => self.community_tasks(&appid),
            None => Task::none(),
        }
    }

    /// Título do jogo/loja atual para o item inicial da coleção.
    fn current_community_title(&self) -> String {
        match &self.screen {
            Screen::Game(id) => self
                .library
                .game(id)
                .map(|game| game.title.clone())
                .unwrap_or_default(),
            Screen::Section(Section::Stores) => self
                .store
                .detail
                .as_ref()
                .map(|detail| detail.game.title.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn open_community_review_form(&mut self, editing: Option<String>) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        let Some(state) = self.community.get_mut(&appid) else {
            return Task::none();
        };
        let form = match editing {
            Some(id) => {
                let Some(review) = state.reviews.items.iter().find(|review| review.id == id) else {
                    return Task::none();
                };
                ReviewFormState::for_review(review)
            }
            None => ReviewFormState::default(),
        };
        state.review_form = Some(form);
        state.collection_form = None;
        state.confirm = None;
        state.report = None;
        state.mutation_error = None;
        state.mutation_notice = None;
        Task::none()
    }

    fn open_community_collection_form(&mut self, editing: Option<String>) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        let Some(state) = self.community.get_mut(&appid) else {
            return Task::none();
        };
        let form = match editing {
            Some(id) => {
                let Some(collection) = state
                    .collections
                    .items
                    .iter()
                    .find(|collection| collection.id == id)
                else {
                    return Task::none();
                };
                CollectionFormState::for_collection(collection)
            }
            None => CollectionFormState::default(),
        };
        state.collection_form = Some(form);
        state.review_form = None;
        state.confirm = None;
        state.report = None;
        state.mutation_error = None;
        state.mutation_notice = None;
        Task::none()
    }

    fn submit_community_review(&mut self) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        let backend = self.backend.clone();
        let payload_appid = appid.clone();
        let message_appid = appid.clone();
        let (payload, action) = {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            if state.mutation_busy {
                return Task::none();
            }
            let Some(form) = state.review_form.as_ref() else {
                return Task::none();
            };
            let draft = match form.validate() {
                Ok(draft) => draft,
                Err(error) => {
                    state.mutation_error = Some(error.message(&self.i18n).to_string());
                    return Task::none();
                }
            };
            let action = if form.editing.is_some() {
                CommunityMutationKind::ReviewUpdate
            } else {
                CommunityMutationKind::ReviewCreate
            };
            let mut payload = json!({
                "appid": payload_appid,
                "text": draft.text,
                "rating": draft.rating,
                "positive": draft.rating >= 3,
                "hours": draft.hours,
            });
            if let Some(title) = draft.title {
                payload["title"] = Value::String(title);
            }
            state.mutation_busy = true;
            state.mutation_error = None;
            state.mutation_notice = None;
            (payload, action)
        };
        let editing = self
            .community
            .get(&appid)
            .and_then(|state| state.review_form.as_ref())
            .and_then(|form| form.editing.clone());
        Task::perform(
            async move {
                match editing {
                    Some(id) => backend.update_community_review(id, payload).await,
                    None => backend.create_community_review(payload).await,
                }
            },
            move |result| Message::CommunityMutationDone {
                appid: message_appid,
                action,
                result,
            },
        )
    }

    fn submit_community_collection(&mut self) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        let game_title = self.current_community_title();
        let backend = self.backend.clone();
        let payload_appid = appid.clone();
        let message_appid = appid.clone();
        let (payload, action) = {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            if state.mutation_busy {
                return Task::none();
            }
            let Some(form) = state.collection_form.as_ref() else {
                return Task::none();
            };
            let draft = match form.validate() {
                Ok(draft) => draft,
                Err(error) => {
                    state.mutation_error = Some(error.message(&self.i18n).to_string());
                    return Task::none();
                }
            };
            let editing = form.editing.is_some();
            let action = if editing {
                CommunityMutationKind::CollectionUpdate
            } else {
                CommunityMutationKind::CollectionCreate
            };
            let mut payload = json!({
                "title": draft.title,
                "description": draft.description,
                "visibility": draft.visibility.as_str(),
            });
            if !editing && !payload_appid.is_empty() {
                payload["items"] = json!([{
                    "appid": payload_appid,
                    "title": game_title,
                }]);
            }
            state.mutation_busy = true;
            state.mutation_error = None;
            state.mutation_notice = None;
            (payload, action)
        };
        let editing = self
            .community
            .get(&appid)
            .and_then(|state| state.collection_form.as_ref())
            .and_then(|form| form.editing.clone());
        Task::perform(
            async move {
                match editing {
                    Some(id) => backend.update_community_collection(id, payload).await,
                    None => backend.create_community_collection(payload).await,
                }
            },
            move |result| Message::CommunityMutationDone {
                appid: message_appid,
                action,
                result,
            },
        )
    }

    fn toggle_community_mine(&mut self, mine: bool) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            if state.mine_only == mine {
                return Task::none();
            }
            state.mine_only = mine;
            state.collections = Default::default();
            state.mutation_error = None;
            state.mutation_notice = None;
        }
        self.load_community_collections(&appid, 0, false)
    }

    fn start_community_item(
        &mut self,
        action: CommunityMutationKind,
        collection_id: String,
    ) -> Task<Message> {
        if !self.account.is_signed_in() {
            return Task::none();
        }
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        if appid.is_empty() {
            return Task::none();
        }
        let backend = self.backend.clone();
        let message_appid = appid.clone();
        {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            if state.mutation_busy {
                return Task::none();
            }
            state.mutation_busy = true;
            state.mutation_error = None;
            state.mutation_notice = None;
        }
        Task::perform(
            async move {
                match action {
                    CommunityMutationKind::CollectionItemAdd => {
                        backend
                            .add_community_collection_item(collection_id, appid)
                            .await
                    }
                    _ => {
                        backend
                            .remove_community_collection_item(collection_id, appid)
                            .await
                    }
                }
            },
            move |result| Message::CommunityMutationDone {
                appid: message_appid,
                action,
                result,
            },
        )
    }

    fn confirm_community_removal(&mut self) -> Task<Message> {
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        let backend = self.backend.clone();
        let message_appid = appid.clone();
        let (confirm, action) = {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            if state.mutation_busy {
                return Task::none();
            }
            let Some(confirm) = state.confirm.clone() else {
                return Task::none();
            };
            let action = match confirm {
                CommunityConfirm::Review(_) => CommunityMutationKind::ReviewRemove,
                CommunityConfirm::Collection(_) => CommunityMutationKind::CollectionRemove,
            };
            state.mutation_busy = true;
            state.mutation_error = None;
            state.mutation_notice = None;
            (confirm, action)
        };
        Task::perform(
            async move {
                match confirm {
                    CommunityConfirm::Review(id) => backend.remove_community_review(id).await,
                    CommunityConfirm::Collection(id) => {
                        backend.remove_community_collection(id).await
                    }
                }
            },
            move |result| Message::CommunityMutationDone {
                appid: message_appid,
                action,
                result,
            },
        )
    }

    fn submit_community_report(&mut self) -> Task<Message> {
        let Some(appid) = self.current_community_appid() else {
            return Task::none();
        };
        let backend = self.backend.clone();
        let message_appid = appid.clone();
        let (report, action) = {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            if state.mutation_busy {
                return Task::none();
            }
            let Some(report) = state.report.clone() else {
                return Task::none();
            };
            let reason = report.reason.trim();
            if reason.is_empty() {
                state.mutation_error = Some(
                    crate::views::community::CommunityFormError::EmptyReportReason
                        .message(&self.i18n)
                        .to_string(),
                );
                return Task::none();
            }
            let action = match report.target {
                CommunityTarget::Review => CommunityMutationKind::ReviewReport,
                CommunityTarget::Collection => CommunityMutationKind::CollectionReport,
            };
            state.mutation_busy = true;
            state.mutation_error = None;
            state.mutation_notice = None;
            (
                CommunityReportState {
                    reason: reason.to_string(),
                    ..report
                },
                action,
            )
        };
        Task::perform(
            async move {
                match report.target {
                    CommunityTarget::Review => {
                        backend
                            .report_community_review(report.id, report.reason)
                            .await
                    }
                    CommunityTarget::Collection => {
                        backend
                            .report_community_collection(report.id, report.reason)
                            .await
                    }
                }
            },
            move |result| Message::CommunityMutationDone {
                appid: message_appid,
                action,
                result,
            },
        )
    }

    fn finish_community_mutation(
        &mut self,
        appid: String,
        action: CommunityMutationKind,
        result: Result<CommunityMutation, CommunityError>,
    ) -> Task<Message> {
        let notice = action.success_key().map(|key| self.i18n.t(key).to_string());
        let reload_offset = {
            let Some(state) = self.community.get_mut(&appid) else {
                return Task::none();
            };
            state.mutation_busy = false;
            match result {
                Ok(_) => {
                    state.mutation_error = None;
                    state.mutation_notice = notice;
                    match action {
                        CommunityMutationKind::ReviewCreate
                        | CommunityMutationKind::ReviewUpdate => {
                            state.review_form = None;
                        }
                        CommunityMutationKind::ReviewRemove => {
                            state.confirm = None;
                        }
                        CommunityMutationKind::ReviewReport => {
                            state.report = None;
                        }
                        CommunityMutationKind::CollectionCreate
                        | CommunityMutationKind::CollectionUpdate => {
                            state.collection_form = None;
                        }
                        CommunityMutationKind::CollectionRemove => {
                            state.confirm = None;
                        }
                        CommunityMutationKind::CollectionReport => {
                            state.report = None;
                        }
                        CommunityMutationKind::CollectionItemAdd
                        | CommunityMutationKind::CollectionItemRemove => {}
                    }
                    Some(if action.is_review() {
                        state.reviews.page_offset
                    } else {
                        state.collections.page_offset
                    })
                }
                Err(error) => {
                    state.mutation_error = Some(error.message);
                    None
                }
            }
        };
        if matches!(
            action,
            CommunityMutationKind::ReviewReport | CommunityMutationKind::CollectionReport
        ) {
            return Task::none();
        }
        match reload_offset {
            Some(offset) if action.is_review() => {
                self.load_community_reviews(&appid, offset, false)
            }
            Some(offset) => self.load_community_collections(&appid, offset, false),
            None => Task::none(),
        }
    }

    /// Aplica uma leitura de `library:get` sem tocar no estado da tela.
    fn apply_library_snapshot(&mut self, mut games: Vec<Game>) -> Task<Message> {
        let mut tasks = Vec::new();
        let mut retro_art = Vec::new();
        for game in &mut games {
            if game.retro_system().is_none() {
                continue;
            }
            if let Some(url) = &game.cover_url {
                retro_art.push((url.clone(), None));
                continue;
            }
            match self.retro_library_art.get(&game.id) {
                Some(Some((url, fallback))) => {
                    game.cover_url = Some(url.clone());
                    retro_art.push((url.clone(), fallback.clone()));
                }
                Some(None) => {}
                None => {
                    let id = game.id.clone();
                    self.retro_library_art.insert(id.clone(), None);
                    let backend = self.backend.clone();
                    let request = id.clone();
                    tasks.push(Task::perform(
                        async move { backend.resolve_retro_cover(request).await },
                        move |art| Message::RetroLibraryArtLoaded { id, art },
                    ));
                }
            }
        }
        self.library.ready(games);
        if self
            .game_menu
            .as_ref()
            .is_some_and(|menu| self.library.game(&menu.game_id).is_none())
        {
            self.game_menu = None;
        }
        self.big_picture
            .sync_games(self.library.visible_all().len());
        let urls = self.library.image_urls();
        tasks.push(self.retro_cover_tasks(retro_art));
        tasks.push(self.cover_tasks(urls));
        Task::batch(tasks)
    }

    /// Ficha aberta de jogo Steam ainda marcado como não instalado: é o único
    /// caso em que vale sondar a Steam de tempos em tempos.
    fn steam_install_pending(&self) -> bool {
        matches!(&self.screen, Screen::Game(id)
            if self.library.game(id).is_some_and(|game| {
                !game.installed && game.launcher.eq_ignore_ascii_case("steam")
            }))
    }

    /// Refresh em segundo plano: sem spinner, sem perder a ficha aberta e sem
    /// duas requisições sobrepostas.
    fn light_library_refresh(&mut self) -> Task<Message> {
        if self.library_poll_in_flight || self.library.status != LibraryStatus::Ready {
            return Task::none();
        }
        let backend = self.backend.clone();
        self.library_dirty = false;
        self.library_poll_in_flight = true;
        self.library_epoch = self.library_epoch.wrapping_add(1);
        let epoch = self.library_epoch;
        Task::perform(
            async move { backend.load_library().await },
            move |result| Message::LibraryRefreshed { epoch, result },
        )
    }

    fn reload_task(&self) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(
            async move { backend.load_library().await },
            Message::LibraryLoaded,
        )
    }

    fn game_media_task(&self, game: &Game) -> Task<Message> {
        let backend = self.backend.clone();
        let id = game.id.clone();
        let title = game.title.clone();
        let message_id = game.id.clone();
        Task::perform(
            async move { backend.load_game_media(id, title).await },
            move |media| Message::MediaLoaded {
                game_id: message_id,
                media,
            },
        )
    }

    /// Dispara, em paralelo, os painéis da ficha para o jogo. O mapa
    /// `game_panels` é o cache: se o jogo já foi aberto nesta sessão, nada é
    /// refeito. Jogos fora da Steam/Retro entram como `Ready` com uma nota
    /// explícita, não como painel vazio.
    fn game_panels_task(&mut self, game: &Game) -> Task<Message> {
        if self.game_panels.contains_key(&game.id) {
            return Task::none();
        }

        let mut panels = GamePanels::default();
        let mut tasks = Vec::new();

        if let Some(appid) = game.steam_appid().map(str::to_string) {
            panels.achievements.status = LoadStatus::Loading;
            panels.proton.status = LoadStatus::Loading;
            panels.stats.status = LoadStatus::Loading;

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            let achievements_appid = appid.clone();
            tasks.push(Task::perform(
                async move { backend.load_steam_achievements(achievements_appid).await },
                move |result| Message::AchievementsLoaded { game_id, result },
            ));

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            let proton_appid = appid.clone();
            tasks.push(Task::perform(
                async move { backend.load_protondb(proton_appid).await },
                move |result| Message::ProtonDbLoaded { game_id, result },
            ));

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            tasks.push(Task::perform(
                async move { backend.load_game_stats(appid).await },
                move |result| Message::GameStatsLoaded { game_id, result },
            ));
        } else if let (true, Some(system)) = (game.is_retro(), game.retro_system()) {
            panels.achievements.status = LoadStatus::Loading;
            panels.proton.status = LoadStatus::Ready;
            panels.proton.note = Some(self.i18n.t("native.game.note.retro_proton").to_string());
            panels.stats.status = LoadStatus::Ready;
            panels.stats.note = Some(self.i18n.t("native.game.note.retro_stats").to_string());

            let backend = self.backend.clone();
            let game_id = game.id.clone();
            let title = game.title.clone();
            let system = system.to_string();
            tasks.push(Task::perform(
                async move { backend.load_retro_achievements(title, system).await },
                move |result| Message::AchievementsLoaded { game_id, result },
            ));
        } else {
            let launcher = if game.launcher.trim().is_empty() {
                self.i18n.t("native.game.launcher_unknown")
            } else {
                game.launcher.as_str()
            };
            panels.achievements.status = LoadStatus::Ready;
            panels.achievements.note = Some(self.i18n.t_args(
                "native.game.note.achievements_launcher",
                &[("launcher", launcher)],
            ));
            panels.proton.status = LoadStatus::Ready;
            panels.proton.note = Some(
                self.i18n
                    .t("native.game.note.proton_steam_only")
                    .to_string(),
            );
            panels.stats.status = LoadStatus::Ready;
            panels.stats.note = Some(self.i18n.t("native.game.note.stats_steam_only").to_string());
        }

        self.game_panels.insert(game.id.clone(), panels);
        Task::batch(tasks)
    }

    /// Painéis ProtonDB/Stats/Conquistas da ficha da loja. Reaproveita o mesmo
    /// cache da biblioteca, com uma chave `store:` própria.
    fn store_panels_task(&mut self, game: &StoreGame) -> Task<Message> {
        let game_id = store_panels_id(game);
        if self.game_panels.contains_key(&game_id) {
            return Task::none();
        }

        let appid = game.appid.trim().to_string();
        let mut panels = GamePanels::default();
        let mut tasks = Vec::new();

        if appid.is_empty() || !appid.chars().all(|character| character.is_ascii_digit()) {
            panels.achievements.status = LoadStatus::Ready;
            panels.proton.status = LoadStatus::Ready;
            panels.stats.status = LoadStatus::Ready;
            panels.achievements.note = Some(
                self.i18n
                    .t("native.game.note.achievements_launcher")
                    .to_string(),
            );
            panels.proton.note = Some(
                self.i18n
                    .t("native.game.note.proton_steam_only")
                    .to_string(),
            );
            panels.stats.note = Some(self.i18n.t("native.game.note.stats_steam_only").to_string());
            self.game_panels.insert(game_id, panels);
            return Task::none();
        }

        panels.achievements.status = LoadStatus::Loading;
        panels.proton.status = LoadStatus::Loading;
        panels.stats.status = LoadStatus::Loading;

        let backend = self.backend.clone();
        let achievements_id = game_id.clone();
        let achievements_appid = appid.clone();
        tasks.push(Task::perform(
            async move { backend.load_steam_achievements(achievements_appid).await },
            move |result| Message::AchievementsLoaded {
                game_id: achievements_id,
                result,
            },
        ));

        let backend = self.backend.clone();
        let proton_id = game_id.clone();
        let proton_appid = appid.clone();
        tasks.push(Task::perform(
            async move { backend.load_protondb(proton_appid).await },
            move |result| Message::ProtonDbLoaded {
                game_id: proton_id,
                result,
            },
        ));

        let backend = self.backend.clone();
        let stats_id = game_id.clone();
        tasks.push(Task::perform(
            async move { backend.load_game_stats(appid).await },
            move |result| Message::GameStatsLoaded {
                game_id: stats_id,
                result,
            },
        ));

        self.game_panels.insert(game_id, panels);
        Task::batch(tasks)
    }

    fn desktop_store_visible(&self) -> bool {
        matches!(self.screen, Screen::Section(Section::Stores))
            && self.store.tab == StoreTab::Steam && self.store.detail.is_none()
    }

    fn console_store_visible(&self) -> bool {
        matches!(self.screen, Screen::BigPicture) && self.big_picture.tab() == ConsoleTab::Stores
    }

    fn store_thumbnail_tasks(&mut self, urls: impl IntoIterator<Item = String>) -> Task<Message> {
        for url in urls {
            if !self.store_thumbnail_queue.contains(&url) {
                self.store_thumbnail_queue.push_back(url);
            }
        }
        self.run_store_thumbnail_queue()
    }

    fn run_store_thumbnail_queue(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while self.store_thumbnail_jobs < STORE_THUMBNAIL_JOBS {
            let Some(url) = self.store_thumbnail_queue.pop_front() else { break };
            self.store_thumbnails.touch(&url);
            if !self.store_thumbnails.request(&url) {
                continue;
            }
            self.store_thumbnail_jobs += 1;
            let backend = self.backend.clone();
            let fetch_url = url.clone();
            tasks.push(Task::perform(
                async move { backend.fetch_store_thumbnail(fetch_url).await },
                move |image| Message::StoreThumbnailLoaded { url, image },
            ));
        }
        Task::batch(tasks)
    }

    fn retro_cover_tasks(
        &mut self,
        art: impl IntoIterator<Item = (String, Option<String>)>,
    ) -> Task<Message> {
        let tasks: Vec<_> = art.into_iter().filter_map(|(url, fallback)| {
            if !self.covers.request(&url) {
                return None;
            }
            let backend = self.backend.clone();
            let request = url.clone();
            Some(Task::perform(
                async move { backend.fetch_retro_card(request, fallback).await },
                move |loaded| Message::CoverLoaded { url, loaded },
            ))
        }).collect();
        Task::batch(tasks)
    }

    fn cover_tasks(&mut self, urls: Vec<String>) -> Task<Message> {
        let mut tasks = Vec::new();
        let foreground = self.store.detail.is_some()
            || matches!(self.screen, Screen::Game(_))
            || (matches!(self.screen, Screen::BigPicture) && self.big_picture.overview_open());

        for url in urls {
            if !self.covers.request(&url) {
                continue;
            }

            let backend = self.backend.clone();
            let fetch_url = url.clone();
            tasks.push(Task::perform(
                async move { backend.fetch_cover_priority(fetch_url, foreground).await },
                move |loaded| Message::CoverLoaded { url, loaded },
            ));
        }

        Task::batch(tasks)
    }
}

fn store_thumbnail_urls(games: &[StoreGame]) -> Vec<String> {
    games.iter()
        .take(STORE_THUMBNAIL_AHEAD)
        .filter_map(|game| game.cover_url.clone())
        .collect()
}

fn verification_summary(i18n: &I18n, verification: &PluginVerification) -> String {
    if !verification.ok || !verification.valid {
        return if verification.error.is_empty() {
            i18n.t("native.components.verify_failed_no_detail")
                .to_string()
        } else {
            i18n.t_args(
                "native.components.verify_failed_detail",
                &[("error", &verification.error)],
            )
        };
    }
    if verification.verified {
        i18n.t("native.components.verify_ok_digest").to_string()
    } else if verification.declared {
        i18n.t("native.components.verify_ok_declared").to_string()
    } else {
        i18n.t("native.components.verify_ok_unsigned").to_string()
    }
}

/// Horário curto da última checagem de update, exibido na aba Sobre.
fn now_label() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

/// Teclado do console: mesmas intenções do gamepad (`NavEvent`).
fn console_shelf_for<'a>(library: &'a Library, tab: ConsoleTab) -> Vec<&'a Game> {
    let games = library.visible_all();
    if tab == ConsoleTab::Home {
        home_shelf(&games).0
    } else {
        games
    }
}

/// Patch do menu para `overrides:set`. Booleano desejado, nunca `null`; e a
/// lista de categorias sempre explícita (`[]` quando todas foram removidas).
fn library_override_patch(game: &Game) -> serde_json::Value {
    json!({
        "favorite": game.favorite,
        "hidden": game.hidden,
        "categories": game.categories,
    })
}

fn shorten_error(error: &str, limit: usize) -> String {
    let mut text: String = error.chars().take(limit).collect();
    if error.chars().count() > limit {
        text.push('…');
    }
    text
}

fn picker_error_text(i18n: &I18n, error: &str) -> String {
    format!("{} {error}", i18n.t("native.picker.error"))
}

fn source_error_text(i18n: &I18n, error: &str) -> String {
    let code = error.strip_prefix("sources:add: ").unwrap_or(error);
    let key = match code {
        "cloudflare_challenge" => "native.sources.cloudflare",
        "source_json_invalid" => "native.sources.json_invalid",
        "source_json_too_large" => "native.sources.json_too_large",
        "source_json_read_failed" => "native.sources.json_read_failed",
        _ => return error.to_string(),
    };
    i18n.t(key).to_string()
}

fn rom_folder_error_text(i18n: &I18n, error: RomFolderAddError) -> String {
    let key = match error {
        RomFolderAddError::Empty => "native.emulation.folder_invalid",
        RomFolderAddError::Limit => "native.emulation.folder_limit",
        RomFolderAddError::Duplicate => "native.emulation.folder_duplicate",
    };
    i18n.t(key).to_string()
}

fn account_error_text(i18n: &I18n, code: &str) -> String {
    // Erros de `dispatch` chegam como "[api_unavailable] … (canal)"; o código
    // entre colchetes é o que o mapa de mensagens entende.
    let code = code
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map(|(code, _)| code)
        .unwrap_or(code);
    let key = match code {
        "credenciais_invalidas" => "account.erro_credenciais",
        "username_invalido" => "account.erro_username",
        "username_ocupado" => "account.erro_ocupado",
        "email_invalido" => "account.erro_email",
        "senha_curta" => "account.erro_senha_curta",
        "confirmacao_necessaria" => "account.erro_confirmacao",
        "rede_indisponivel" => "account.erro_rede",
        "api_unavailable" => "native.account.erro_api",
        "nao_logado" => "native.account.erro_nao_logado",
        "avatar_ilegivel" | "avatar_nao_imagem" | "avatar_grande" => "native.account.erro_avatar",
        "arquivo_ilegivel" => "native.account.erro_arquivo",
        "arquivo_grande" => "native.account.erro_imagem_grande",
        "sem_campos" => "native.account.erro_sem_campos",
        "resposta_invalida" => "native.account.erro_resposta",
        // Sessão expirada: o servidor recusa o refresh token e devolve
        // "Invalid Refresh Token: Refresh Token Not Found" — que o mapa
        // desconhecia, caindo em "Operation failed (texto cru em inglês)".
        // Aqui vira uma mensagem que diz o que fazer: entrar de novo.
        other if is_expired_session(other) => "native.account.erro_sessao_expirada",
        other => return format!("{} ({other})", i18n.t("account.erro_geral")),
    };
    i18n.t(key).to_string()
}

/// `true` quando o erro do servidor é sessão expirada/refresh token inválido.
///
/// O texto vem do Supabase e varia ("Invalid Refresh Token: Refresh Token Not
/// Found", "refresh_token_not_found", "JWT expired"); o que importa é o
/// significado, não a redação.
fn is_expired_session(code: &str) -> bool {
    let lower = code.to_ascii_lowercase();
    lower.contains("refresh token")
        || lower.contains("refresh_token")
        || lower.contains("jwt expired")
        || lower.contains("token has expired")
}

fn console_nav_from_key(event: &keyboard::Event) -> Option<NavEvent> {
    let keyboard::Event::KeyPressed { key, .. } = event else {
        return None;
    };

    use keyboard::key::Named;

    match key {
        keyboard::Key::Named(Named::ArrowUp) => Some(NavEvent::Up),
        keyboard::Key::Named(Named::ArrowDown) => Some(NavEvent::Down),
        keyboard::Key::Named(Named::ArrowLeft) => Some(NavEvent::Left),
        keyboard::Key::Named(Named::ArrowRight) => Some(NavEvent::Right),
        keyboard::Key::Named(Named::Enter) | keyboard::Key::Named(Named::Space) => {
            Some(NavEvent::Confirm)
        }
        keyboard::Key::Named(Named::Escape) => Some(NavEvent::Back),
        keyboard::Key::Named(Named::PageUp) => Some(NavEvent::PageLeft),
        keyboard::Key::Named(Named::PageDown) => Some(NavEvent::PageRight),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn retro_cached_page_stays_visible_on_failed_refresh() {
        let root = std::env::temp_dir().join(format!("arcadia-retro-preview-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let generation = app.retro.generation;
        let page = RetroPage {
            games: vec![RetroGame { id: "retro:test:123".into(), title: "Fixture".into(), ..Default::default() }],
            ..Default::default()
        };
        drop(app.update(Message::RetroPageCached {
            generation, offset: 0, payload: json!({}), page: Some(page),
        }));
        assert_eq!(app.retro.status, LoadStatus::Ready);
        drop(app.update(Message::RetroPageLoaded {
            generation, offset: 0, result: Err("offline".into()),
        }));
        assert_eq!(app.retro.status, LoadStatus::Ready);
        assert_eq!(app.retro.games[0].title, "Fixture");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn automatic_retro_refresh_keeps_the_current_page_and_survives_failure() {
        let root = std::env::temp_dir().join(format!("arcadia-retro-revalidate-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        app.screen = Screen::Section(Section::Stores);
        app.store.tab = StoreTab::Retro;
        app.retro.offset = RETRO_PAGE_SIZE;
        app.retro.games.push(RetroGame { title: "Página atual".into(), ..Default::default() });
        app.retro.status = LoadStatus::Ready;
        app.section_loaded_at.clear();

        assert_eq!(app.update(Message::RefreshVisibleSection).units(), 1);
        assert_eq!(app.retro.offset, RETRO_PAGE_SIZE);
        assert_eq!(app.update(Message::RefreshVisibleSection).units(), 0);
        drop(app.update(Message::RetroPageLoaded {
            generation: app.retro.generation,
            offset: RETRO_PAGE_SIZE,
            result: Err("offline".into()),
        }));
        assert_eq!(app.retro.status, LoadStatus::Ready);
        assert_eq!(app.retro.games[0].title, "Página atual");
        assert_eq!(app.update(Message::RefreshVisibleSection).units(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retro_next_page_can_supersede_a_refresh_and_loads_its_covers_from_cache() {
        let root = std::env::temp_dir().join(format!("arcadia-retro-page-change-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        app.screen = Screen::Section(Section::Stores);
        app.store.tab = StoreTab::Retro;
        app.retro.status = LoadStatus::Ready;
        app.retro.has_more = true;
        app.section_loaded_at.clear();

        drop(app.update(Message::RefreshVisibleSection));
        let old_generation = app.retro.generation;
        assert_eq!(app.retro.status, LoadStatus::Ready);
        assert_eq!(app.update(Message::RetroNextPage).units(), 1);
        let generation = app.retro.generation;
        assert_ne!(generation, old_generation);
        let url = "https://example.test/page-2.jpg";
        let tasks = app.update(Message::RetroPageCached {
            generation,
            offset: RETRO_PAGE_SIZE,
            payload: Backend::retro_page_params("", "", RETRO_PAGE_SIZE, RETRO_PAGE_SIZE),
            page: Some(RetroPage {
                games: vec![RetroGame { title: "Página 2".into(), cover_url: Some(url.into()), ..Default::default() }],
                ..Default::default()
            }),
        });
        // Cache hit queues the new cover, scrolls to the top, and revalidates.
        assert_eq!(tasks.units(), 3);
        assert_eq!(app.retro.offset, RETRO_PAGE_SIZE);
        assert!(!app.covers.request(url));
        drop(app.update(Message::RetroPageLoaded {
            generation: old_generation,
            offset: 0,
            result: Ok(RetroPage::default()),
        }));
        assert_eq!(app.retro.games[0].title, "Página 2");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn store_preview_respects_pagination_and_refresh() {
        let root = std::env::temp_dir().join(format!("arcadia-store-preview-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        app.screen = Screen::Section(Section::Stores);

        let game = |id: u64| StoreGame { appid: id.to_string(), ..StoreGame::default() };
        let first_page = StorePage {
            games: vec![game(10)],
            has_more: true,
            next_offset: Some(24),
            ..StorePage::default()
        };
        let next_page = StorePage { games: vec![game(20)], ..StorePage::default() };
        let generation = app.store.browse.generation;
        drop(app.update(Message::StoreBrowseLoaded { offset: 0, generation, result: Ok(first_page.clone()) }));
        assert_eq!(app.update(Message::StorePrefetchNext).units(), 1);
        assert_eq!(app.update(Message::StorePrefetchNext).units(), 0);
        drop(app.update(Message::StoreBrowseLoaded { offset: 24, generation, result: Ok(next_page.clone()) }));
        assert_eq!(app.store.browse.games.len(), 1);
        assert_eq!(app.update(Message::StoreLoadMore).units(), 0);
        assert_eq!(app.store.browse.games.len(), 2);
        assert!(!app.store.browse.has_more);

        drop(app.load_store_browse(true));
        drop(app.update(Message::StoreBrowseLoaded { offset: 24, generation, result: Ok(next_page) }));
        assert!(app.store.browse.next_page.is_none());
        assert_eq!(app.store.browse.status, LoadStatus::Loading);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn source_import_cancel_keeps_url_and_errors_explain_browser_verification() {
        let root = std::env::temp_dir().join(format!("arcadia-source-import-ui-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        app.sources.status = LoadStatus::Ready;
        app.sources.url_draft = "https://hydralinks.cloud/sources/fitgirl.json".into();
        let purpose = PickPurpose::SourceCatalog { url: app.sources.url_draft.clone() };
        drop(app.update(Message::PickRequested(purpose.clone())));
        assert!(app.sources.busy);
        drop(crate::views::sources::view(&app));
        drop(app.update(Message::PickDone { purpose, result: Ok(None) }));
        assert!(!app.sources.busy);
        assert!(!app.sources.url_draft.is_empty());
        drop(app.update(Message::SourcesActionDone(Err("sources:add: cloudflare_challenge".into()))));
        assert_eq!(app.sources.action_error.as_deref(), Some(app.i18n.t("native.sources.cloudflare")));
        assert!(!app.sources.url_draft.is_empty());
        assert_eq!(source_error_text(&app.i18n, "source_json_invalid"), app.i18n.t("native.sources.json_invalid"));
        assert_eq!(source_error_text(&app.i18n, "HTTP 500"), "HTTP 500");
    }

    #[test]
    fn live_download_progress_merges_queues_and_rejects_stale_refresh_results() {
        let mut state = DownloadsState::default();
        let stale = crate::backend::parse_downloads(
            Some(json!([{ "appid": "steam:10", "status": "downloading", "percent": 5 }])),
            Some(json!({ "downloads": [{ "gameId": "tor:20", "engine": "http", "progress": 0.1 }] })),
        );
        let before = state.revisions;
        state.busy_id = Some("steam:10".into());
        let dm = crate::backend::parse_downloads(
            Some(json!([{ "appid": "steam:10", "status": "paused", "percent": 50 }])), None,
        );
        state.apply_progress(false, dm.items);
        state.apply_snapshot(before, stale.clone());
        let steam = state.data.items.iter().find(|item| !item.is_torrent).unwrap();
        assert_eq!(steam.percent, 50.0);
        assert_eq!(steam.status, crate::backend::DownloadStatus::Paused);
        assert_eq!(state.data.items.len(), 2);
        assert_eq!(state.busy_id.as_deref(), Some("steam:10"));
        let before_cancel = state.revisions;
        state.apply_progress(true, Vec::new());
        state.apply_snapshot(before_cancel, stale);
        assert_eq!(state.data.items.len(), 1, "uma leitura atrasada restaurou o download cancelado");
        assert_eq!(state.data.items[0].id, "steam:10");
        assert_eq!(state.status, LoadStatus::Ready);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn downloads_update_from_service_events_without_refreshing_the_page() {
        let root = std::env::temp_dir().join(format!("arcadia-native-live-download-ui-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("torrent_state.json"), json!([{
            "gameId": "tor:20", "title": "Fixture", "engine": "http",
            "url": "https://example.test/fixture.bin", "progress": 0.4,
        }]).to_string()).unwrap();
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend.clone());
        drop(boot);
        backend.run_download_action("tor:20".into(), true, DownloadAction::Pause).await.unwrap();
        app.downloads.busy_id = Some("tor:20".into());
        drop(app.update(Message::LaunchEventsTick));
        assert_eq!(app.downloads.data.items[0].percent, 40.0);
        assert_eq!(app.downloads.data.items[0].status, crate::backend::DownloadStatus::Paused);
        assert_eq!(app.downloads.busy_id.as_deref(), Some("tor:20"));
        backend.run_download_action("tor:20".into(), true, DownloadAction::Cancel).await.unwrap();
        drop(app.update(Message::LaunchEventsTick));
        assert!(app.downloads.data.items.is_empty(), "a tela exigiu Refresh após cancelar");
        assert_eq!(app.downloads.status, LoadStatus::Ready);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn source_downloads_work_without_a_steam_manifest_or_library_and_ignore_stale_results() {
        let root = std::env::temp_dir().join(format!("arcadia-source-downloads-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, task) = App::boot(backend);
        drop(task);
        let game = StoreGame { appid: "1174180".into(), title: "Red Dead Redemption 2".into(), ..Default::default() };
        drop(app.open_store_detail(game.clone()));
        let generation = app.store.detail.as_ref().unwrap().downloads.generation;
        drop(app.update(Message::StoreInstallInfoLoaded {
            appid: game.appid.clone(), result: Err("nenhum provedor disponível".into()),
        }));
        let offer = crate::backend::SourceOffer {
            reference: "source:0".into(), title: game.title.clone(), source: "Source".into(),
            file_size: "10 GB".into(), uris: vec!["https://example.test/game.zip".into()],
        };
        let data = SourceOffers { offers: vec![offer], unavailable_sources: vec![] };
        drop(app.update(Message::StoreSourceOffersLoaded {
            appid: game.appid.clone(), generation, cached_only: true, result: Ok(data.clone()),
        }));
        let detail = app.store.detail.as_ref().unwrap();
        assert!(crate::views::store_downloads::available(detail));
        assert!(detail.libraries.is_empty());
        assert_eq!(detail.downloads.status, LoadStatus::Loading);
        drop(app.update(Message::StoreDownloadRequested));
        assert!(app.store.detail.as_ref().unwrap().downloads.open);
        assert!(!app.store.detail.as_ref().unwrap().install_busy, "abrir o diálogo iniciou o download");
        drop(crate::views::store::detail_view(&app, app.store.detail.as_ref().unwrap()));
        drop(crate::views::store_downloads::overlay(&app, app.store.detail.as_ref().unwrap()));
        drop(app.update(Message::StoreDownloadPathChanged("   ".into())));
        drop(app.update(Message::StoreSourceDownloadConfirmed));
        assert!(!app.store.detail.as_ref().unwrap().install_busy);
        drop(app.update(Message::StoreDownloadPathChanged("/chosen/games".into())));
        drop(app.update(Message::StoreSourceDownloadConfirmed));
        assert!(app.store.detail.as_ref().unwrap().install_busy);
        drop(app.update(Message::StoreSourceDownloadDone {
            appid: game.appid.clone(), generation, result: Err("hoster_unavailable".into()),
        }));
        assert!(!app.store.detail.as_ref().unwrap().install_busy);
        assert!(app.store.detail.as_ref().unwrap().downloads.open);
        drop(app.update(Message::StoreSourceOffersLoaded {
            appid: game.appid.clone(), generation, cached_only: false, result: Ok(data.clone()),
        }));
        assert_eq!(app.store.detail.as_ref().unwrap().downloads.error.as_deref(), Some("hoster_unavailable"));
        drop(app.open_store_detail(game.clone()));
        let current = app.store.detail.as_ref().unwrap().downloads.generation;
        assert_ne!(generation, current);
        drop(app.update(Message::StoreSourceOffersLoaded {
            appid: game.appid.clone(), generation, cached_only: false, result: Ok(data.clone()),
        }));
        assert!(app.store.detail.as_ref().unwrap().downloads.data.offers.is_empty());
        drop(app.update(Message::StoreSourceOffersLoaded {
            appid: game.appid.clone(), generation: current, cached_only: false, result: Ok(data),
        }));
        drop(app.update(Message::StoreSourceOffersLoaded {
            appid: game.appid.clone(), generation: current, cached_only: true, result: Ok(SourceOffers::default()),
        }));
        assert!(crate::views::store_downloads::available(app.store.detail.as_ref().unwrap()));
        drop(app.update(Message::StoreDownloadRequested));
        drop(app.update(Message::StoreSourceDownloadDone {
            appid: game.appid, generation: current, result: Ok(()),
        }));
        assert!(!app.store.detail.as_ref().unwrap().downloads.open);
        assert!(app.store.detail.as_ref().unwrap().install_message.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn profile_starts_avatar_animation_before_static_download_finishes() {
        let data_dir = std::env::temp_dir()
            .join(format!("profile-avatar-start-test-{}", std::process::id()));
        let backend = Backend::new(
            arcadia_ipc::AppState::new(data_dir),
            tokio::runtime::Handle::current(),
        );
        let (mut app, initial_tasks) = App::boot(backend);
        drop(initial_tasks);
        app.account.apply_session(AccountSession::default());
        let url = "https://example.test/avatar.gif";
        drop(app.update(Message::AccountProfileLoaded(Ok(AccountProfile {
            avatar_url: Some(url.to_string()),
            ..AccountProfile::default()
        }))));
        assert_eq!(app.account.avatar_url.as_deref(), Some(url));
        assert!(app.account.avatar.is_none());
        assert!(
            !app.animated_cards.wants(url),
            "animação ficou esperando AccountAvatarLoaded",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn navigating_preserves_profile_animation_and_pending_downloads() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                encoder.encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
                    0, 0, image::Delay::from_numer_denom_ms(20, 1),
                )).expect("GIF válido");
            }
        }
        let media = crate::animated_cards::decode_media(bytes).await.expect("mídia válida");
        let data_dir = std::env::temp_dir()
            .join(format!("profile-navigation-test-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(data_dir), tokio::runtime::Handle::current());
        let (mut app, initial_tasks) = App::boot(backend);
        drop(initial_tasks);
        for uri in ["avatar.gif", "banner.gif", "background.gif"] {
            app.animated_cards.insert_decoded(uri, media.clone(), crate::animated_cards::MediaGroup::Profile);
        }
        app.animated_cards.mark_in_flight("new-background.gif");
        for section in [Section::Library, Section::Home, Section::Settings] {
            drop(app.update(Message::Navigate(section)));
            drop(app.update(Message::CardsTick));
            for uri in ["avatar.gif", "banner.gif", "background.gif"] {
                assert!(app.animated_cards.texture(uri).is_some(), "{uri} desapareceu em {section:?}");
                assert!(!app.animated_cards.wants(uri), "{uri} exige novo download");
            }
            assert!(!app.animated_cards.wants("new-background.gif"), "perdeu download em andamento");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn console_detail_builds_all_tabs_and_opens_screenshot() {
        let path = std::env::temp_dir().join(format!("console-detail-test-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(path), tokio::runtime::Handle::current());
        let (mut app, tasks) = App::boot(backend);
        drop(tasks);
        let game = game("custom:detail", "Fixture game", false, false);
        app.library.ready(vec![game.clone()]);
        app.screen = Screen::BigPicture;
        app.big_picture.sync_games(1);
        drop(app.console_nav(NavEvent::Down));
        assert!(app.big_picture.overview_open());
        drop(app.update(Message::ConsoleMediaLoaded { id: game.id.clone(), media: GameMedia {
            screenshots: vec![crate::backend::Screenshot { thumb: Some("https://example.test/shot.jpg".into()), full: Some("https://example.test/full.jpg".into()) }],
            req_min: Some("<p>4 GB RAM</p>".into()),
            ..GameMedia::default()
        }}));
        for tab in crate::big_picture::OverviewTab::ALL {
            drop(app.update(Message::ConsoleOverviewTab(tab)));
            assert_eq!(app.big_picture.overview_tab(), tab);
            drop(views::big_picture::view(&app));
        }
        drop(app.update(Message::ConsoleOverviewTab(crate::big_picture::OverviewTab::Media)));
        drop(app.update(Message::ConsoleOverviewMedia(0)));
        assert!(app.big_picture.overview_preview());
        assert!(!app.gallery_trailer);
        drop(views::big_picture::view(&app));
        drop(app.update(Message::ConsoleOverviewClosePreview));
        assert!(app.big_picture.overview_open());
        assert!(!app.big_picture.overview_preview());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn storefront_is_console_only_and_opens_the_visible_game() {
        let root=std::env::temp_dir().join(format!("arcadia-storefront-test-{}",std::process::id()));
        let backend=Backend::new(arcadia_ipc::AppState::new(root),tokio::runtime::Handle::current());
        let (mut app,task)=App::boot(backend);drop(task);
        let mut catalog=Vec::new();
        for index in 1..=12 { catalog.push(StoreGame{appid:index.to_string(),title:format!("Game {index}"),genres:vec!["Action".into()],..Default::default()}); }
        app.store.browse.games=catalog.clone();app.store.browse.status=LoadStatus::Ready;app.store.browse.has_more=true;
        app.home.status=LoadStatus::Ready;
        let desktop:Vec<_>=app.console_store_games().iter().map(|g|g.appid.clone()).collect();
        drop(app.update(Message::ConsoleStoreCategory(3)));
        assert_eq!(app.console_storefront.category,0,"mensagem de console alterou desktop");
        app.screen=Screen::BigPicture;
        app.big_picture.select_tab(ConsoleTab::Stores,&[]);
        app.console_storefront.language=Some(Lang::PtBr);
        let feed=crate::backend::StorefrontFeed {specials:catalog[..5].to_vec(),releases:catalog[5..10].to_vec(),popular:catalog[10..].to_vec(),..Default::default()};
        drop(app.update(Message::ConsoleStorefrontLoaded {language:Lang::PtBr,result:Ok(feed)}));
        assert_eq!(app.console_store_games().len(),10);
        for category in 0..9 {
            drop(app.update(Message::ConsoleStoreCategory(category)));
            for (width,height) in [(800.,600.),(1150.,755.),(1920.,1080.)] {app.window_size=Size::new(width,height);drop(views::big_picture::view(&app));}
        }
        drop(app.update(Message::ConsoleStoreCategory(0)));
        app.big_picture.focus_store_game(1);
        drop(app.console_nav(NavEvent::Confirm));
        assert_eq!(app.store.detail.as_ref().map(|d|d.game.appid.as_str()),Some("2"));
        assert!(app.big_picture.store_detail_open());
        drop(app.console_nav(NavEvent::Back));
        assert!(!app.big_picture.store_detail_open());
        assert_eq!(app.big_picture.store_index(),1);
        drop(app.update(Message::ConsoleStoreHeroStep(1)));
        drop(app.update(Message::ConsoleStoreHeroOpen));
        assert_eq!(app.store.detail.as_ref().map(|d|d.game.appid.as_str()),Some("2"));
        drop(app.console_nav(NavEvent::Back));
        drop(app.update(Message::ConsoleStoreCatalog));
        assert_eq!(app.console_store_games().len(),12);
        assert!(app.big_picture.store_has_more());
        app.screen=Screen::Section(Section::Stores);
        let after:Vec<_>=app.console_store_games().iter().map(|g|g.appid.clone()).collect();
        assert_eq!(after,desktop);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn storefront_filters_cached_genres_and_ignores_stale_language() {
        let root=std::env::temp_dir().join(format!("arcadia-storefront-genres-{}",std::process::id()));
        let backend=Backend::new(arcadia_ipc::AppState::new(root),tokio::runtime::Handle::current());
        let (mut app,task)=App::boot(backend);drop(task);
        app.screen=Screen::BigPicture;app.big_picture.select_tab(ConsoleTab::Stores,&[]);
        app.store.browse.games=vec![StoreGame{appid:"1".into(),title:"Game".into(),..Default::default()}];
        app.cache_store_info("1",StoreInfo{genres:vec!["Ação".into(),"Indie".into()],..Default::default()});
        app.console_storefront.category=3;assert_eq!(app.console_store_games().len(),1);
        app.console_storefront.category=6;assert_eq!(app.console_store_games().len(),1);
        app.console_storefront.category=8;assert!(app.console_store_games().is_empty());
        app.console_storefront.language=Some(Lang::EnUs);
        drop(app.update(Message::ConsoleStorefrontLoaded{language:Lang::PtBr,result:Ok(Default::default())}));
        assert_eq!(app.console_storefront.language,Some(Lang::EnUs));
    }

    #[tokio::test(flavor="current_thread")]
    async fn friends_port_opens_remote_games_without_overwriting_local_playtime() {
        let root=std::env::temp_dir().join(format!("arcadia-friends-port-{}",std::process::id()));
        let backend=Backend::new(arcadia_ipc::AppState::new(root),tokio::runtime::Handle::current());
        let (mut app,task)=App::boot(backend);drop(task);
        app.account.apply_session(AccountSession{id:"self".into(),username:Some("me".into()),..Default::default()});
        app.screen=Screen::Section(Section::Friends);
        let people:Vec<_>=(0..40).map(|index|crate::backend::FriendItem{id:format!("friend-{index}"),username:format!("Friend {index}"),
            display_name:None,avatar_url:Some(format!("https://example.test/avatar-{index}.png")),since:Some("2026-11-08T00:00:00Z".into())}).collect();
        drop(app.update(Message::FriendsLoaded(Ok(FriendsLoad::Ready(crate::backend::FriendsData{friends:people,incoming:vec![],outgoing:vec![]})))));
        assert!(matches!(&app.friends.data,Some(FriendsLoad::Ready(data)) if data.friends.len()==40));
        assert!(!app.covers.request("https://example.test/avatar-39.png"),"último avatar não foi solicitado");
        let local=Game::from_value(serde_json::json!({"id":"steam:578080","title":"PUBG","cover":"https://example.test/local.jpg","playtime_minutes":9999})).unwrap();
        app.library.games=vec![local];app.library.status=LibraryStatus::Ready;
        let original=app.library.games.clone();
        for id in ["friend-0","friend-20","friend-39"] {
            drop(app.update(Message::FriendProfileRequested(id.into())));
            assert_eq!(app.friends.profile.as_ref().unwrap().id,id);
            assert!(app.friends.profile.as_ref().unwrap().is_friend);
        }
        let remote=crate::backend::parse_friend_profile(serde_json::json!({"ok":true,
            "profile":{"username":"friend","banner_url":"https://example.test/friend-banner.gif"},
            "games":[{"appid":"steam:578080","title":"PUBG","minutes":135},{"appid":"10","title":"Another","minutes":240}],
            "friends":[{"id":"self","username":"me"}],
            "stats":{"jogos":2,"playtime_hours":6}})).unwrap();
        drop(app.update(Message::FriendProfileLoaded{id:"friend-0".into(),result:Ok(remote.clone())}));
        assert_eq!(app.friends.profile.as_ref().unwrap().profile.as_ref().unwrap().display(),"Friend 39","resposta antiga substituiu perfil");
        drop(app.update(Message::FriendProfileLoaded{id:"friend-39".into(),result:Ok(remote)}));
        let profile=app.friends.profile.as_ref().unwrap().profile.as_ref().unwrap();
        assert_eq!(profile.library[0].id,"steam:10");
        assert_eq!(profile.library[1].playtime_minutes,135);
        assert_eq!(profile.library[1].cover_url.as_deref(),Some("https://example.test/local.jpg"));
        assert_eq!(app.library.games,original);
        assert!(!app.friend_media.wants("https://example.test/friend-banner.gif"));
        for size in [Size::new(1024.,640.),Size::new(1352.,887.),Size::new(1920.,1080.)] {
            app.window_size=size;drop(views::friends::view(&app));
        }
        drop(app.update(Message::FriendRemoveConfirmation(true)));
        assert!(app.friends.profile.as_ref().unwrap().confirming_remove);
        assert!(app.friends.action_busy.is_none(),"pedir confirmação removeu amizade");
        drop(app.update(Message::FriendRemoveConfirmation(false)));
        drop(app.update(Message::FriendProfileClosed));
        assert!(app.friends.profile.is_none());assert_eq!(app.library.games,original);
        drop(views::friends::view(&app));
    }

    #[tokio::test(flavor="current_thread")]
    async fn friends_port_search_discards_stale_results_and_empty_query() {
        let root=std::env::temp_dir().join(format!("arcadia-friends-search-{}",std::process::id()));
        let backend=Backend::new(arcadia_ipc::AppState::new(root),tokio::runtime::Handle::current());
        let (mut app,task)=App::boot(backend);drop(task);
        app.account.apply_session(AccountSession::default());
        drop(app.update(Message::FriendSearchChanged("alice".into())));
        drop(app.update(Message::FriendSearchReady("alice".into())));
        assert_eq!(app.friends.search_requested.as_deref(),Some("alice"));
        drop(app.update(Message::FriendSearchChanged("bob".into())));
        drop(app.update(Message::FriendSearchDone{query:"alice".into(),result:Ok(vec![])}));
        assert!(app.friends.search_results.is_none());
        drop(app.update(Message::FriendSearchReady("bob".into())));
        drop(app.update(Message::FriendSearchDone{query:"bob".into(),result:Ok(vec![FriendSearchResult{id:"bob".into(),..Default::default()}])}));
        assert_eq!(app.friends.search_results.as_ref().unwrap()[0].id,"bob");
        drop(app.update(Message::FriendSearchChanged(String::new())));
        drop(app.update(Message::FriendSearchDone{query:"bob".into(),result:Ok(vec![])}));
        assert!(app.friends.search_results.is_none());assert_eq!(app.friends.search_status,LoadStatus::Idle);
    }

    #[tokio::test(flavor="current_thread")]
    async fn friends_avatar_starts_in_list_before_profile_is_opened() {
        let root=std::env::temp_dir().join(format!("arcadia-friends-avatar-list-{}",std::process::id()));
        let backend=Backend::new(arcadia_ipc::AppState::new(root),tokio::runtime::Handle::current());
        let (mut app,task)=App::boot(backend);drop(task);
        app.account.apply_session(AccountSession::default());app.screen=Screen::Section(Section::Friends);
        let person=crate::backend::FriendItem{id:"friend".into(),username:"Friend".into(),display_name:None,avatar_url:Some("https://example.test/list-avatar.gif".into()),since:None};
        drop(app.update(Message::FriendsLoaded(Ok(FriendsLoad::Ready(crate::backend::FriendsData{friends:vec![person],incoming:vec![],outgoing:vec![]})))));
        assert!(app.friends.profile.is_none());
        assert!(!app.friend_media.wants("https://example.test/list-avatar.gif"),"avatar animado ficou esperando abrir o perfil");
        let mut bytes=Vec::new();
        {
            let mut encoder=image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255,0,0,255],[0,0,255,255]] {
                encoder.encode_frame(image::Frame::from_parts(image::RgbaImage::from_pixel(8,8,image::Rgba(color)),0,0,
                    image::Delay::from_numer_denom_ms(100,1))).unwrap();
            }
        }
        let media=crate::animated_cards::decode_avatar(bytes).await.unwrap();
        let url="https://example.test/list-avatar.gif";
        drop(app.update(Message::FriendAvatarLoaded{url:url.into(),media:Some(media)}));
        let first=app.friend_avatar_texture(url).expect("avatar na lista").id();
        let started=Instant::now();
        while app.friend_avatar_texture(url).unwrap().id()==first && started.elapsed()<Duration::from_secs(1) {
            tokio::time::sleep(Duration::from_millis(5)).await;
            drop(app.update(Message::CardsTick));
        }
        assert_ne!(app.friend_avatar_texture(url).unwrap().id(),first,"quadros não avançam na lista sem perfil aberto");
        drop(app.update(Message::FriendProfileRequested("friend".into())));
        assert!(app.friend_avatar_texture(url).is_some(),"abrir perfil removeu avatar pronto");
        drop(app.update(Message::FriendProfileClosed));
        assert!(app.friend_avatar_texture(url).is_some(),"voltar à lista removeu animação");
    }

    #[tokio::test(flavor="current_thread")]
    async fn friends_open_profile_shell_immediately_without_achievement_interstitial() {
        let root=std::env::temp_dir().join(format!("arcadia-friends-direct-profile-{}",std::process::id()));
        let backend=Backend::new(arcadia_ipc::AppState::new(root),tokio::runtime::Handle::current());
        let (mut app,task)=App::boot(backend);drop(task);app.account.apply_session(AccountSession::default());
        app.friends.data=Some(FriendsLoad::Ready(crate::backend::FriendsData{friends:vec![crate::backend::FriendItem{id:"friend".into(),username:"friend".into(),display_name:Some("Friend name".into()),avatar_url:None,since:None}],incoming:vec![],outgoing:vec![]}));
        drop(app.update(Message::FriendProfileRequested("friend".into())));
        let state=app.friends.profile.as_ref().unwrap();
        assert_eq!(state.status,LoadStatus::Loading);
        assert_eq!(state.profile.as_ref().map(|profile|profile.display()),Some("Friend name"),"abriu a tela antiga enquanto aguardava os jogos");
        assert_eq!(state.achievements_status,LoadStatus::Idle);
        assert!(state.achievements.is_none());
        drop(views::friends::view(&app));
        drop(app.update(Message::FriendProfileLoaded{id:"friend".into(),result:Err("offline".into())}));
        assert!(matches!(app.friends.profile.as_ref().unwrap().status,LoadStatus::Error(_)));
        assert_eq!(app.friends.profile.as_ref().unwrap().profile.as_ref().unwrap().display(),"Friend name");
        drop(views::friends::view(&app));
    }

    fn test_i18n() -> I18n {
        I18n::new(crate::i18n::Lang::PtBr)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prefetched_details_open_synchronously_and_late_results_do_not_replace_current_game() {
        let root = std::env::temp_dir().join(format!("arcadia-prefetch-test-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, task) = App::boot(backend);
        drop(task);
        let store_game = StoreGame { appid: "730".into(), title: "Store fixture".into(), ..Default::default() };
        let info = StoreInfo { description: Some("Cached description".into()), about: Some("Cached about".into()), ..Default::default() };
        drop(app.update(Message::PrefetchedStore { appid: "730".into(), result: Ok(info.clone()) }));
        assert!(app.store.detail.is_none(), "prefetch abriu uma ficha");
        assert!(!app.media.is_active(), "prefetch iniciou um trailer");
        let before = app.store_info_cache["730"].0;
        let start = Instant::now();
        drop(app.open_store_detail(store_game.clone()));
        println!("CACHED_STORE_OPEN_US={}", start.elapsed().as_micros());
        assert!(app.store.detail.is_some(), "a ficha não abriu no clique");
        assert!(!app.media.is_active(), "o clique montou o pipeline do trailer");
        drop(app.update(Message::StoreDetailLoaded { appid: "730".into(), result: Ok(info.clone()) }));
        assert_eq!(app.store.detail.as_ref().unwrap().info.as_ref(), Some(&info));
        assert!(matches!(app.store.detail.as_ref().unwrap().status, LoadStatus::Ready));
        assert_eq!(app.store_info_cache["730"].0, before, "reabertura renovou TTL indevidamente");
        drop(app.update(Message::StoreDetailLoaded { appid: "570".into(), result: Ok(StoreInfo::default()) }));
        assert_eq!(app.store.detail.as_ref().unwrap().info.as_ref(), Some(&info));
        let game = game("custom:cached", "Library fixture", false, false);
        app.library.ready(vec![game.clone()]);
        let media = GameMedia {
            about: Some("Cached library description".into()),
            movies: vec![crate::backend::Movie {
                name: "Trailer".into(),
                thumb: None,
                hls: Some("https://example.invalid/trailer.m3u8".into()),
                mp4: None,
                webm: None,
            }],
            ..Default::default()
        };
        drop(app.update(Message::PrefetchedGame { id: game.id.clone(), media: media.clone() }));
        let start = Instant::now();
        drop(app.open_game(game.id.clone()));
        println!("CACHED_LIBRARY_OPEN_US={}", start.elapsed().as_micros());
        assert!(matches!(&app.screen, Screen::Game(id) if id == &game.id), "a ficha não abriu no clique");
        assert!(app.game_media.is_none(), "o clique aplicou a mídia em cache na hora");
        assert!(!app.media.is_active(), "o clique montou o pipeline do trailer");
        drop(app.update(Message::MediaLoaded { game_id: game.id.clone(), media: media.clone() }));
        assert_eq!(app.game_media.as_ref(), Some(&media));
        drop(app.update(Message::MediaLoaded { game_id: "custom:late".into(), media: GameMedia { about: Some("wrong".into()), ..Default::default() } }));
        assert_eq!(app.game_media.as_ref(), Some(&media));
        drop(app.enter_big_picture());
        assert!(app.game_media.is_none(), "entrar no console manteve o pipeline da ficha");
        drop(app.console_nav(NavEvent::Down));
        assert!(app.big_picture.overview_open());
        assert!(app.game_media.is_none(), "o console aplicou a mídia em cache no frame da navegação");
        drop(app.update(Message::ConsoleMediaLoaded { id: game.id.clone(), media: media.clone() }));
        assert_eq!(app.game_media.as_ref(), Some(&media));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prefetch_is_bounded_deduplicated_and_released_on_failure() {
        let root = std::env::temp_dir().join(format!("arcadia-prefetch-bounds-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root), tokio::runtime::Handle::current());
        let (mut app, task) = App::boot(backend);
        drop(task);
        for id in ["1", "1", "2", "3"] {
            drop(app.prefetch_store(StoreGame { appid: id.into(), ..Default::default() }));
        }
        assert_eq!(app.detail_prefetching.len(), 2);
        drop(app.update(Message::PrefetchedStore { appid: "1".into(), result: Err("offline".into()) }));
        assert_eq!(app.detail_prefetching.len(), 1);
        assert!(!app.store_info_cache.contains_key("1"));
        drop(app.prefetch_store(StoreGame { appid: "3".into(), ..Default::default() }));
        assert_eq!(app.detail_prefetching.len(), 2);
    }

    /// O erro real que o servidor manda quando a sessão morre. Antes ele caía
    /// no ramo genérico e a UI mostrava "Operation failed (Invalid Refresh
    /// Token: ...)" — texto cru em inglês que não diz o que fazer.
    #[test]
    fn expired_session_error_becomes_an_actionable_message() {
        let i18n = test_i18n();
        let message = account_error_text(&i18n, "Invalid Refresh Token: Refresh Token Not Found");
        assert!(
            !message.contains("Operation failed"),
            "caiu no genérico: {message}"
        );
        assert_eq!(message, i18n.t("native.account.erro_sessao_expirada"));
    }

    /// O código entre colchetes do `dispatch` é o que o mapa entende; o
    /// envelope inteiro não deve confundir a detecção.
    #[test]
    fn expired_session_is_detected_inside_a_dispatch_envelope() {
        let i18n = test_i18n();
        assert!(is_expired_session("refresh_token_not_found"));
        assert!(is_expired_session("JWT expired"));
        // Um erro que não é de sessão continua no ramo genérico.
        assert!(!is_expired_session("profile_missing"));
        let _ = &i18n;
    }

    #[test]
    fn library_override_patch_expresses_booleans_and_empty_categories() {
        let mut game = game("steam:440", "Team Fortress 2", true, true);
        game.categories.clear();
        let patch = library_override_patch(&game);
        assert_eq!(patch["favorite"], true);
        assert_eq!(patch["hidden"], true);
        assert_eq!(patch["categories"], json!([]));
        assert!(!patch["favorite"].is_null());
        assert!(!patch["hidden"].is_null());

        game.favorite = false;
        game.hidden = false;
        let patch = library_override_patch(&game);
        assert_eq!(patch["favorite"], false);
        assert_eq!(patch["hidden"], false);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn context_menu_routes_by_stable_id_and_closes_on_actions() {
        let root = std::env::temp_dir().join(format!("arcadia-context-menu-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);

        let mut steam = game("steam:440", "Team Fortress 2", false, false);
        steam.launcher = "steam".to_string();
        steam.installed = true;
        app.library.ready(vec![steam.clone()]);

        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            steam.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        drop(app.update(Message::GameMenuAction(MenuAction::Details)));
        assert!(matches!(&app.screen, Screen::Game(id) if id == &steam.id));
        assert!(app.game_menu.is_none());

        app.screen = Screen::Library;
        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            steam.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        drop(app.update(Message::GameMenuAction(MenuAction::Settings)));
        assert!(app.launch_options.open);
        assert_eq!(app.launch_options.game_id, steam.id);
        assert!(app.game_menu.is_none());

        app.launch_options.open = false;
        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            steam.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        drop(app.update(Message::GameMenuAction(MenuAction::Primary)));
        assert_eq!(app.launch.status, GameRunStatus::Launching);
        assert_eq!(app.launch.game_id.as_deref(), Some(steam.id.as_str()));
        assert!(app.game_menu.is_none());

        // Rota de instalação Steam: a Task pode ser derrubada no teste sem abrir
        // a Steam, e o estado de launch não pode ser tocado.
        app.launch = GameLaunchState::default();
        steam.installed = false;
        app.library.ready(vec![steam.clone()]);
        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            steam.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        drop(app.update(Message::GameMenuAction(MenuAction::Primary)));
        assert_eq!(app.launch.status, GameRunStatus::Idle);
        assert!(app.launch.game_id.is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn menu_mutations_are_optimistic_and_ignore_stale_responses() {
        let root = std::env::temp_dir().join(format!("arcadia-menu-overrides-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);

        let mut game = game("steam:440", "Team Fortress 2", false, false);
        game.launcher = "steam".to_string();
        game.categories = vec!["Ação".into()];
        app.library.ready(vec![game.clone()]);
        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            game.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        drop(app.update(Message::GameMenuAction(MenuAction::ToggleFavorite)));
        let current = app.library.game(&game.id).unwrap();
        assert!(current.favorite, "favorito não mudou na hora");
        assert!(!current.hidden);
        assert!(app.game_menu.is_none());
        assert!(app.library_overrides.busy);
        assert_eq!(app.library_overrides.game_id.as_deref(), Some(game.id.as_str()));
        let epoch = app.library_overrides.epoch;

        // Resposta antiga (epoch anterior) não pode limpar o busy nem mexer.
        drop(app.update(Message::GameOverridesSaved {
            epoch: epoch.wrapping_sub(1),
            account: app.current_account_id(),
            game_id: game.id.clone(),
            result: Ok(()),
        }));
        assert!(app.library_overrides.busy);

        drop(app.update(Message::GameOverridesSaved {
            epoch,
            account: app.current_account_id(),
            game_id: game.id.clone(),
            result: Ok(()),
        }));
        assert!(!app.library_overrides.busy);

        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            game.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        app.game_menu.as_mut().unwrap().categories_open = true;
        drop(app.update(Message::GameMenuCategoryToggled("Ação".into())));
        let current = app.library.game(&game.id).unwrap();
        assert!(current.categories.is_empty(), "remover a única categoria falhou");
        assert!(app.library_overrides.busy);
        drop(app.update(Message::GameOverridesSaved {
            epoch: app.library_overrides.epoch,
            account: app.current_account_id(),
            game_id: game.id.clone(),
            result: Err("disk full".into()),
        }));
        assert!(!app.library_overrides.busy);
        assert!(
            app.game_menu
                .as_ref()
                .and_then(|menu| menu.notice.as_ref())
                .is_some(),
            "falha de override ficou muda no menu aberto"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_metadata_response_does_not_close_another_game() {
        let root = std::env::temp_dir().join(format!("arcadia-menu-metadata-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let game = game("steam:440", "Team Fortress 2", false, false);
        app.library.ready(vec![game.clone()]);
        app.open_game_metadata(&game.id);
        app.game_metadata.as_mut().unwrap().request = 4;
        drop(app.update(Message::GameMetadataSaved {
            game_id: game.id.clone(),
            request: 3,
            account: app.current_account_id(),
            result: Ok(()),
        }));
        assert!(app.game_metadata.is_some(), "resposta antiga fechou o modal");
        drop(app.update(Message::GameMetadataSaved {
            game_id: "steam:570".into(),
            request: 4,
            account: app.current_account_id(),
            result: Ok(()),
        }));
        assert!(app.game_metadata.is_some(), "resposta de outro jogo fechou o modal");
        drop(app.update(Message::GameMetadataSaved {
            game_id: game.id.clone(),
            request: 4,
            account: app.current_account_id(),
            result: Ok(()),
        }));
        assert!(app.game_metadata.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn launch_options_overlay_is_available_over_the_library() {
        let root = std::env::temp_dir().join(format!("arcadia-menu-launch-options-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let game = game("steam:440", "Team Fortress 2", false, false);
        app.library.ready(vec![game.clone()]);
        app.screen = Screen::Library;
        app.launch_options.open = true;
        app.launch_options.game_id = game.id.clone();
        app.launch_options.game_title = game.title.clone();
        drop(crate::views::overlays::stack(
            &app,
            iced::widget::Space::new().into(),
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn escape_closes_the_menu_and_the_metadata_modal() {
        let root = std::env::temp_dir().join(format!("arcadia-menu-escape-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let game = game("steam:440", "Team Fortress 2", false, false);
        app.library.ready(vec![game.clone()]);
        app.game_menu = Some(crate::views::game_menu::GameMenuState::new(
            game.id.clone(),
            iced::Point::new(20.0, 20.0),
        ));
        drop(app.update(Message::GameMenuKey(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })));
        assert!(app.game_menu.is_none());
        app.open_game_metadata(&game.id);
        drop(app.update(Message::GameMenuKey(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })));
        assert!(app.game_metadata.is_none());
        drop(app.update(Message::CloseGameContextMenu));
        assert!(app.game_menu.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn platform_picklist_label_is_persisted_as_value() {
        let root = std::env::temp_dir().join(format!("arcadia-platform-pick-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let mut custom = game("custom:fixture", "Fixture", false, false);
        custom.platform = Some("windows".into());
        custom.exe = Some("/games/fixture.exe".into());
        app.library.ready(vec![custom.clone()]);
        app.open_game_edit(&custom.id);
        // O pick_list entrega "Linux" (rótulo), não "linux" (valor).
        let platform = crate::views::game_edit::EditPlatform::from_label("Linux")
            .expect("rótulo do pick_list não convertido");
        drop(app.update(Message::GameEditPlatformChanged(platform)));
        let edit = app.game_edit.as_ref().unwrap();
        assert_eq!(
            edit.platform,
            Some(crate::views::game_edit::EditPlatform::Linux)
        );
        assert_eq!(edit.payload().custom.platform.as_deref(), Some("linux"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_menu_results_do_not_close_another_or_reopened_menu() {
        let root = std::env::temp_dir().join(format!("arcadia-menu-instance-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let a = game("steam:440", "A", false, false);
        let b = game("steam:570", "B", false, false);
        app.library.ready(vec![a.clone(), b.clone()]);

        drop(app.update(Message::OpenGameContextMenu {
            game_id: a.id.clone(),
            anchor: iced::Point::new(10.0, 10.0),
        }));
        let instance_a = app.game_menu.as_ref().unwrap().instance;
        drop(app.update(Message::OpenGameContextMenu {
            game_id: b.id.clone(),
            anchor: iced::Point::new(20.0, 20.0),
        }));
        let instance_b = app.game_menu.as_ref().unwrap().instance;
        assert_ne!(instance_a, instance_b);

        drop(app.update(Message::GameMenuLogsDone {
            game_id: a.id.clone(),
            instance: instance_a,
            result: Ok(GameLogOutcome::Opened),
        }));
        assert_eq!(app.game_menu.as_ref().map(|menu| menu.game_id.as_str()), Some(b.id.as_str()));
        drop(app.update(Message::GameMenuInstallDone {
            game_id: a.id.clone(),
            instance: instance_a,
            result: Ok(()),
        }));
        assert_eq!(app.game_menu.as_ref().map(|menu| menu.game_id.as_str()), Some(b.id.as_str()));
        assert!(app.game_menu.as_ref().unwrap().notice.is_none());

        // Resultado atual de B fecha normalmente.
        drop(app.update(Message::GameMenuInstallDone {
            game_id: b.id.clone(),
            instance: instance_b,
            result: Ok(()),
        }));
        assert!(app.game_menu.is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn right_click_message_cannot_open_a_menu_behind_modals() {
        let root = std::env::temp_dir().join(format!("arcadia-menu-modal-{}", std::process::id()));
        let backend = Backend::new(arcadia_ipc::AppState::new(root.clone()), tokio::runtime::Handle::current());
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let game = game("steam:440", "A", false, false);
        app.library.ready(vec![game.clone()]);

        app.open_game_edit(&game.id);
        drop(app.update(Message::OpenGameContextMenu {
            game_id: game.id.clone(),
            anchor: iced::Point::new(10.0, 10.0),
        }));
        assert!(app.game_menu.is_none(), "menu abriu atrás do editor");
        app.game_edit = None;

        app.open_game_metadata(&game.id);
        drop(app.update(Message::OpenGameContextMenu {
            game_id: game.id.clone(),
            anchor: iced::Point::new(10.0, 10.0),
        }));
        assert!(app.game_menu.is_none(), "menu abriu atrás dos metadados");
        app.game_metadata = None;

        app.launch_options.open = true;
        app.launch_options.game_id = game.id.clone();
        drop(app.update(Message::OpenGameContextMenu {
            game_id: game.id.clone(),
            anchor: iced::Point::new(10.0, 10.0),
        }));
        assert!(app.game_menu.is_none(), "menu abriu atrás das opções de launch");
        let _ = std::fs::remove_dir_all(root);
    }

    fn game(id: &str, title: &str, favorite: bool, hidden: bool) -> Game {
        Game {
            id: id.to_string(),
            title: title.to_string(),
            launcher: "custom".to_string(),
            launch_cmd: Vec::new(),
            cover_url: None,
            hero_url: None,
            logo_url: None,
            description: None,
            developer: None,
            publisher: None,
            genre: None,
            year: None,
            platform: None,
            exe: None,
            playtime_minutes: 0,
            installed: true,
            hidden,
            favorite,
            categories: Vec::new(),
        }
    }

    #[test]
    fn visible_games_hide_and_filter_then_sort_favorites_first() {
        let mut library = Library::loading();
        library.ready(vec![
            game("1", "Zelda", false, false),
            game("2", "Alpha", false, false),
            game("3", "Beta", true, false),
            game("4", "Oculto", true, true),
        ]);

        let titles: Vec<&str> = library
            .visible_games("")
            .into_iter()
            .map(|game| game.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Beta", "Alpha", "Zelda"]);

        let filtered: Vec<&str> = library
            .visible_games("ALP")
            .into_iter()
            .map(|game| game.title.as_str())
            .collect();
        assert_eq!(filtered, vec!["Alpha"]);

        assert!(library.game("4").is_some());
        assert_eq!(library.visible_all().len(), 3);
    }

    /// O agrupamento por plataforma é o que o Orbit faz na Library: cada
    /// plataforma vira uma seção própria, na ordem fixa, sem grupo vazio.
    #[test]
    fn grouped_games_separates_platforms_in_a_fixed_order() {
        let mut steam = game("steam:10", "Counter-Strike", false, false);
        steam.launcher = "steam".to_string();
        let mut epic = game("epic:1", "Fortnite", false, false);
        epic.launcher = "epic".to_string();
        let mut retro = game("retro:ps2:SLUS-1", "Shadow", false, false);
        retro.launcher = "retro".to_string();
        let mut other = game("custom:9", "Meu Jogo", false, false);
        other.launcher = "custom".to_string();

        let mut library = Library::loading();
        library.ready(vec![steam, epic, retro, other]);

        // Na aba "Todos", retro fica de fora por desenho — ele tem aba própria.
        // Os rótulos são copiados antes de mudar a aba: `grouped_games` devolve
        // `&Game` emprestado de `library`.
        {
            let groups = library.grouped_games("");
            let labels: Vec<String> = groups.iter().map(|(label, _)| label.clone()).collect();
            assert_eq!(labels, vec!["steam", "epic", ""]);
            assert_eq!(groups[0].1[0].title, "Counter-Strike");
        }

        // Na aba Retro, só ele aparece, no grupo "retro".
        library.tab = LibraryTab::Retro;
        {
            let retro_groups = library.grouped_games("");
            let retro_labels: Vec<String> = retro_groups
                .iter()
                .map(|(label, _)| label.clone())
                .collect();
            assert_eq!(retro_labels, vec!["retro"]);
        }
    }

    /// Grupo vazio não entra: um cabeçalho "Retro 0" só ocupa altura.
    #[test]
    fn grouped_games_skips_empty_platforms() {
        let mut steam = game("steam:10", "Counter-Strike", false, false);
        steam.launcher = "steam".to_string();

        let mut library = Library::loading();
        library.ready(vec![steam]);

        let groups = library.grouped_games("");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "steam");
    }

    /// A aba separa retro do resto: "Todos os jogos" não pode listar retro, e
    /// a aba Retro não pode listar o resto. Sem o filtro as duas mostravam a
    /// mesma grade.
    #[test]
    fn library_tabs_separate_retro_from_the_rest() {
        let mut retro = game("retro:ps2:SLUS-1", "Shadow of the Colossus", false, false);
        retro.launcher = "retro".to_string();
        // Jogo retro importado: id próprio, marcado pelo launcher.
        let mut imported = game("9", "Crash", false, false);
        imported.launcher = "retro".to_string();

        let mut library = Library::loading();
        library.ready(vec![
            game("steam:10", "Counter-Strike", false, false),
            retro,
            imported,
        ]);

        library.tab = LibraryTab::All;
        let todos: Vec<&str> = library
            .visible_games("")
            .into_iter()
            .map(|game| game.title.as_str())
            .collect();
        assert_eq!(todos, vec!["Counter-Strike"]);

        library.tab = LibraryTab::Retro;
        let retro_only: Vec<&str> = library
            .visible_games("")
            .into_iter()
            .map(|game| game.title.as_str())
            .collect();
        assert_eq!(retro_only.len(), 2);
        assert!(retro_only.contains(&"Shadow of the Colossus"));
        assert!(retro_only.contains(&"Crash"));
    }

    #[test]
    fn section_e_abas_de_configuracoes_seguem_o_idioma() {
        let pt = I18n::new(Lang::PtBr);
        let en = I18n::new(Lang::EnUs);
        let es = I18n::new(Lang::EsEs);

        assert_eq!(Section::Library.label(&pt), "Biblioteca");
        assert_eq!(Section::Library.label(&en), "Library");
        assert_eq!(Section::Settings.label(&en), "Settings");
        assert_eq!(Section::Home.label(&es), "Inicio");
        assert_eq!(SettingsTab::General.label(&en), "General Settings");
        assert_eq!(SettingsTab::Accessibility.label(&es), "Accesibilidad");
        assert_eq!(SettingsTab::Controller.label(&en), "Controller");
        assert_eq!(SettingsTab::Emulation.label(&pt), "Emulação");
        assert_eq!(SettingsTab::Emulation.label(&en), "Emulation");
    }

    #[test]
    fn section_navigation_routes_library_and_placeholders() {
        assert_eq!(Screen::for_section(Section::Library), Screen::Library);
        assert_eq!(
            Screen::for_section(Section::Downloads),
            Screen::Section(Section::Downloads)
        );

        let mut screen = Screen::Game("steam:10".to_string());
        screen.back_to_library();
        assert_eq!(screen, Screen::Library);
    }

    #[test]
    fn image_urls_dedupe_cover_and_hero() {
        let mut library = Library::loading();
        let mut first = game("1", "A", false, false);
        first.cover_url = Some("https://example.test/a.jpg".to_string());
        first.hero_url = Some("https://example.test/a.jpg".to_string());
        let mut second = game("2", "B", false, false);
        second.cover_url = Some("https://example.test/b.jpg".to_string());
        library.ready(vec![first, second]);

        assert_eq!(
            library.image_urls(),
            vec![
                "https://example.test/a.jpg".to_string(),
                "https://example.test/b.jpg".to_string()
            ]
        );
    }

    #[test]
    fn launch_events_drive_the_status_without_polling() {
        let mut launch = GameLaunchState::default();

        let starting = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "starting", "gameId": "steam:10", "token": 1 }),
        )
        .expect("starting");
        launch.apply_event(starting, &test_i18n());
        assert!(launch.is_launching("steam:10"));
        assert!(!launch.is_running("steam:10"));

        let warning = crate::backend::parse_launch_event(
            "game:launchWarning",
            &json!({ "gameId": "steam:10", "warnings": ["gamemoderun ausente"] }),
        )
        .expect("warning");
        launch.apply_event(warning, &test_i18n());
        assert_eq!(launch.warnings_for("steam:10"), ["gamemoderun ausente"]);
        assert!(launch.warnings_for("steam:11").is_empty());

        let running =
            crate::backend::parse_launch_event("game:running", &json!(true)).expect("running");
        launch.apply_event(running, &test_i18n());
        assert!(launch.is_running("steam:10"));

        // `game:running(false)` é a transição natural de fim do monitor.
        let stopped =
            crate::backend::parse_launch_event("game:running", &json!(false)).expect("stopped");
        launch.apply_event(stopped, &test_i18n());
        assert_eq!(launch.status, GameRunStatus::Idle);
        assert!(launch.game_id.is_none());

        // Os avisos continuam disponíveis para o jogo depois do fim.
        assert_eq!(launch.warnings_for("steam:10"), ["gamemoderun ausente"]);
    }

    #[test]
    fn active_and_idle_events_carry_the_game_id() {
        let mut launch = GameLaunchState::default();
        let active = crate::backend::parse_launch_event(
            "game:active",
            &json!({ "rodando": true, "gameId": "steam:10" }),
        )
        .expect("active");
        launch.apply_event(active, &test_i18n());
        assert!(launch.is_running("steam:10"));

        let idle = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "idle", "gameId": "steam:10", "token": 1 }),
        )
        .expect("idle");
        launch.apply_event(idle, &test_i18n());
        assert_eq!(launch.status, GameRunStatus::Idle);
        assert!(launch.game_id.is_none());
    }

    #[test]
    fn stale_events_from_a_previous_session_do_not_cancel_a_new_launch() {
        let mut launch = GameLaunchState::default();

        let starting = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "starting", "gameId": "steam:11" }),
        )
        .expect("starting novo");
        launch.apply_event(starting, &test_i18n());
        assert!(launch.is_launching("steam:11"));

        // Monitor do jogo anterior termina depois do novo `starting`.
        let stale_idle = crate::backend::parse_launch_event(
            "game:launchState",
            &json!({ "state": "idle", "gameId": "steam:10" }),
        )
        .expect("idle antigo");
        launch.apply_event(stale_idle, &test_i18n());
        assert!(launch.is_launching("steam:11"));

        let stale_running_off = crate::backend::parse_launch_event("game:running", &json!(false))
            .expect("running false antigo");
        launch.apply_event(stale_running_off, &test_i18n());
        assert!(launch.is_launching("steam:11"));
    }

    #[test]
    fn launch_error_event_returns_to_idle_with_a_message() {
        let mut launch = GameLaunchState {
            status: GameRunStatus::Launching,
            game_id: Some("steam:10".to_string()),
            ..GameLaunchState::default()
        };
        let error = crate::backend::parse_launch_event(
            "game:launchError",
            &json!({ "gameId": "steam:10", "error": "No such file or directory" }),
        )
        .expect("erro");
        launch.apply_event(error, &test_i18n());
        assert_eq!(launch.status, GameRunStatus::Idle);
        let message = launch.error.as_deref().expect("mensagem");
        assert!(message.contains("No such file"), "{message}");
    }

    #[test]
    fn launch_options_mode_is_scoped_to_the_open_game_and_requires_saved_edits() {
        let state = LaunchOptionsState {
            game_id: "steam:10".to_string(),
            mode: Some("exe".to_string()),
            ..LaunchOptionsState::default()
        };
        assert_eq!(state.mode_for("steam:10"), Some("exe"));
        assert_eq!(state.mode_for("steam:11"), None);

        // Rascunho não salvo não vai no `game:launch`: o backend lê o disco.
        let dirty = LaunchOptionsState {
            dirty: true,
            ..state
        };
        assert_eq!(dirty.mode_for("steam:10"), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn launch_options_advanced_starts_closed_and_toggle_preserves_the_draft() {
        let root = std::env::temp_dir().join(format!(
            "arcadia-launch-advanced-{}",
            std::process::id()
        ));
        let backend = Backend::new(
            arcadia_ipc::AppState::new(root.clone()),
            tokio::runtime::Handle::current(),
        );
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let game = game("custom:advanced", "Advanced fixture", false, false);
        app.library.ready(vec![game]);
        app.launch_options.advanced_open = true;
        drop(app.update(Message::OpenLaunchOptions("custom:advanced".into())));
        assert!(
            !app.launch_options.advanced_open,
            "o diálogo reabriu com os ajustes avançados abertos"
        );

        let settings = GameSettingsView {
            exe_path: "/tmp/game.exe".into(),
            game_args: "--windowed".into(),
            ..GameSettingsView::default()
        };
        app.launch_options.status = LoadStatus::Ready;
        app.launch_options.settings = Some(settings.clone());
        app.launch_options.dirty = false;
        drop(app.update(Message::LaunchOptionsAdvancedToggled));
        assert!(app.launch_options.advanced_open);
        assert!(!app.launch_options.dirty);
        assert_eq!(app.launch_options.settings.as_ref(), Some(&settings));

        drop(app.update(Message::LaunchOptionsAdvancedToggled));
        assert!(!app.launch_options.advanced_open);
        assert_eq!(app.launch_options.settings.as_ref(), Some(&settings));
        assert!(!app.launch_options.dirty);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn launch_options_save_result_keeps_edits_made_in_flight() {
        let root = std::env::temp_dir().join(format!(
            "arcadia-launch-save-{}",
            std::process::id()
        ));
        let backend = Backend::new(
            arcadia_ipc::AppState::new(root.clone()),
            tokio::runtime::Handle::current(),
        );
        let (mut app, boot) = App::boot(backend);
        drop(boot);

        let saved = GameSettingsView {
            game_args: "--saved".into(),
            ..GameSettingsView::default()
        };
        app.launch_options = LaunchOptionsState {
            open: true,
            game_id: "custom:save".into(),
            status: LoadStatus::Ready,
            settings: Some(GameSettingsView::default()),
            saving: true,
            ..LaunchOptionsState::default()
        };
        drop(app.update(Message::LaunchOptionsSaved {
            id: "custom:save".into(),
            result: Ok(saved.clone()),
        }));
        assert!(!app.launch_options.saving);
        assert!(app.launch_options.saved);
        assert_eq!(app.launch_options.settings.as_ref(), Some(&saved));

        // Edição feita com o save em voo não pode ser sobrescrita pela resposta.
        let draft = GameSettingsView {
            game_args: "--draft".into(),
            ..GameSettingsView::default()
        };
        app.launch_options = LaunchOptionsState {
            open: true,
            game_id: "custom:save".into(),
            status: LoadStatus::Ready,
            settings: Some(draft.clone()),
            saving: true,
            dirty: true,
            ..LaunchOptionsState::default()
        };
        drop(app.update(Message::LaunchOptionsSaved {
            id: "custom:save".into(),
            result: Ok(saved),
        }));
        assert!(!app.launch_options.saving);
        assert!(!app.launch_options.saved);
        assert!(app.launch_options.dirty);
        assert_eq!(app.launch_options.settings.as_ref(), Some(&draft));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn another_game_cannot_launch_over_a_running_session() {
        let launch = GameLaunchState {
            status: GameRunStatus::Running,
            game_id: Some("steam:10".to_string()),
            ..GameLaunchState::default()
        };
        assert!(launch.busy_with_other("custom:1"));
        assert!(!launch.busy_with_other("steam:10"));
    }

    #[test]
    fn plugin_action_messages_follow_the_registry_semantics() {
        let i18n = I18n::new(crate::i18n::Lang::PtBr);
        assert_eq!(PluginAction::Install.label_i18n(&i18n), "Instalar");
        assert_eq!(PluginAction::Remove.label_i18n(&i18n), "Remover");
        assert_eq!(PluginAction::Install.label(), "Instalar");
        assert_eq!(PluginAction::Remove.label(), "Remover");
        assert!(PluginAction::Remove
            .success_message_i18n(&i18n)
            .contains("preservado"));
        assert!(PluginAction::Unregister
            .success_message_i18n(&i18n)
            .contains("registro"));

        let verified = PluginVerification {
            id: "demo".to_string(),
            ok: true,
            valid: true,
            verified: true,
            declared: true,
            source: "local".to_string(),
            error: String::new(),
        };
        let i18n = I18n::new(crate::i18n::Lang::PtBr);
        assert!(verification_summary(&i18n, &verified).contains("digest"));

        let failed = PluginVerification {
            valid: false,
            error: "digest divergente".to_string(),
            ..verified
        };
        assert!(verification_summary(&i18n, &failed).contains("digest divergente"));
    }

    #[test]
    fn account_state_moves_from_signed_out_through_loading_to_signed_in() {
        let mut account = AccountState::default();
        assert_eq!(account.status, AccountStatus::Unknown);

        account.signed_out();
        assert_eq!(account.status, AccountStatus::SignedOut);
        assert!(!account.is_signed_in());

        account.auth_failed("credenciais_invalidas".to_string());
        assert_eq!(account.status, AccountStatus::SignedOut);
        assert_eq!(account.auth.error.as_deref(), Some("credenciais_invalidas"));
        assert!(!account.auth.sending);

        account.apply_session(AccountSession {
            id: "user-1".to_string(),
            email: Some("ana@example.test".to_string()),
            username: Some("ana".to_string()),
        });
        assert_eq!(account.status, AccountStatus::Loading);
        assert!(account.is_signed_in());
        assert_eq!(account.display_name("Jogador local"), "ana");

        account.profile_ready(AccountProfile {
            username: "ana".to_string(),
            display_name: Some("Ana".to_string()),
            ..AccountProfile::default()
        });
        assert_eq!(account.status, AccountStatus::SignedIn);
        assert_eq!(account.display_name("Jogador local"), "Ana");

        account.profile_failed("rede_indisponivel".to_string());
        assert_eq!(account.status, AccountStatus::Error);
        assert!(account.is_signed_in(), "falha de perfil preserva a sessão");

        account.signed_out();
        assert_eq!(account.status, AccountStatus::SignedOut);
        assert!(!account.is_signed_in());
        assert!(account.profile.is_none());
        assert!(account.avatar.is_none());
    }

    #[test]
    fn required_auth_overlay_ignores_close_until_login() {
        let mut account = AccountState::default();
        account.open_auth(true);
        assert!(account.auth.open);
        assert!(!account.close_auth());
        assert!(account.auth.open);

        account.auth.required = false;
        assert!(account.close_auth());
        assert!(!account.auth.open);
    }

    #[test]
    fn account_ipc_errors_are_mapped_to_human_messages() {
        let i18n = I18n::new(Lang::PtBr);
        let credentials = account_error_text(&i18n, "credenciais_invalidas");
        assert!(credentials.contains("incorretos"), "{credentials}");
        let api = account_error_text(&i18n, "[api_unavailable] sem cliente (account:signIn)");
        assert!(api.contains("backend"), "{api}");
        let unknown = account_error_text(&i18n, "erro_novo");
        assert!(unknown.contains("erro_novo"), "{unknown}");
        assert!(account_error_text(&i18n, "arquivo_grande").contains("grande"));
    }

    #[test]
    fn pick_purposes_map_to_profile_fields() {
        assert_eq!(
            ProfileImageKind::from_purpose(&PickPurpose::Background),
            Some(ProfileImageKind::Background)
        );
        assert_eq!(
            ProfileImageKind::from_purpose(&PickPurpose::Banner),
            Some(ProfileImageKind::Banner)
        );
        assert_eq!(ProfileImageKind::from_purpose(&PickPurpose::Avatar), None);
        assert_eq!(ProfileImageKind::Background.ipc_kind(), "background");
        assert_eq!(ProfileImageKind::Banner.ipc_kind(), "banner");

        let mut editor = ProfileEditorState::default();
        editor.background_url = "https://manual/background.png".to_string();
        *editor.image_url_mut(ProfileImageKind::Banner) = "https://manual/banner.png".to_string();
        assert_eq!(editor.background_url, "https://manual/background.png");
        assert_eq!(editor.banner_url, "https://manual/banner.png");

        *editor.image_upload_busy_mut(ProfileImageKind::Background) = true;
        assert!(editor.uploading_background);
        assert!(!editor.uploading_banner);
        assert_eq!(
            ProfileImageKind::Banner.success_key(),
            "native.profile.banner_updated"
        );
    }

    #[test]
    fn game_option_pick_fills_the_selected_game_and_emulator_fields() {
        let mut state = LaunchOptionsState {
            open: true,
            game_id: "custom:demo".to_string(),
            settings: Some(GameSettingsView {
                emulator_id: Some("retroarch".to_string()),
                ..GameSettingsView::default()
            }),
            ..LaunchOptionsState::default()
        };

        let rom = PickPurpose::GameRom {
            game_id: "custom:demo".to_string(),
            emulator_id: "retroarch".to_string(),
        };
        assert!(state.apply_pick(&rom, PathBuf::from("/roms/demo.sfc")));
        assert_eq!(state.settings.as_ref().unwrap().rom_path, "/roms/demo.sfc");
        assert!(state.dirty);

        // Resultado atrasado de outro jogo/emulador não altera o rascunho.
        let other_emulator = PickPurpose::GameRom {
            game_id: "custom:demo".to_string(),
            emulator_id: "pcsx2".to_string(),
        };
        assert!(!state.apply_pick(&other_emulator, PathBuf::from("/roms/other.iso")));
        let other_game = PickPurpose::GameCore {
            game_id: "custom:outro".to_string(),
            emulator_id: "retroarch".to_string(),
        };
        assert!(!state.apply_pick(&other_game, PathBuf::from("/cores/outro.so")));

        let core = PickPurpose::GameCore {
            game_id: "custom:demo".to_string(),
            emulator_id: "retroarch".to_string(),
        };
        assert!(state.apply_pick(&core, PathBuf::from("/cores/snes9x.so")));
        let settings = state.settings.as_ref().unwrap();
        assert_eq!(settings.rom_path, "/roms/demo.sfc");
        assert_eq!(settings.emulator_core_path, "/cores/snes9x.so");
    }

    #[test]
    fn emulator_args_text_is_split_and_capped_in_the_game_options() {
        let args = crate::backend::parse_emulator_args(
            &(0..40)
                .map(|index| format!("--flag{index}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        assert_eq!(args.len(), crate::backend::MAX_EMULATOR_ARGS);
        assert_eq!(args[0], "--flag0");
    }

    #[test]
    fn pick_purposes_map_to_emulation_fields_and_rom_folders() {
        let mut editor = EmulationEditorState {
            emulator_id: "pcsx2".to_string(),
            executable: String::new(),
            core_path: "/usr/lib/core.so".to_string(),
            bios_path: String::new(),
            args_text: String::new(),
            rom_folders: Vec::new(),
            folder_draft: String::new(),
            original: None,
            requires_core: true,
            bios_required: true,
            saving: false,
            confirm_remove: false,
            error: None,
            scanning: false,
            scan_error: None,
            preview_available: false,
            preview_roms: Vec::new(),
            preview_truncated: false,
            importing_rom: None,
            notice: None,
            pick_busy: None,
        };

        assert!(editor
            .apply_pick(
                &PickPurpose::EmulatorExecutable("pcsx2".to_string()),
                PathBuf::from("/opt/pcsx2/pcsx2-qt"),
            )
            .unwrap());
        assert_eq!(editor.executable, "/opt/pcsx2/pcsx2-qt");

        assert!(editor
            .apply_pick(
                &PickPurpose::EmulatorCore("pcsx2".to_string()),
                PathBuf::from("/usr/lib/cores/ps2.so"),
            )
            .unwrap());
        assert_eq!(editor.core_path, "/usr/lib/cores/ps2.so");

        assert!(editor
            .apply_pick(
                &PickPurpose::EmulatorBios("pcsx2".to_string()),
                PathBuf::from("/roms/bios"),
            )
            .unwrap());
        assert_eq!(editor.bios_path, "/roms/bios");

        assert!(editor
            .apply_pick(
                &PickPurpose::RomFolder("pcsx2".to_string()),
                PathBuf::from("/roms/ps2"),
            )
            .unwrap());
        assert_eq!(editor.rom_folders.len(), 1);
        assert_eq!(editor.rom_folders[0].path, "/roms/ps2");
        assert!(editor.rom_folders[0].recursive);

        assert_eq!(
            editor.apply_pick(
                &PickPurpose::RomFolder("pcsx2".to_string()),
                PathBuf::from("/roms/ps2"),
            ),
            Err(RomFolderAddError::Duplicate)
        );

        // Resultado atrasado de outro emulador e ROM única (sem campo hoje)
        // não mexem no editor.
        assert!(!editor
            .apply_pick(
                &PickPurpose::EmulatorCore("retroarch".to_string()),
                PathBuf::from("/cores/snes.so"),
            )
            .unwrap());
        assert_eq!(editor.core_path, "/usr/lib/cores/ps2.so");
        assert!(!editor
            .apply_pick(
                &PickPurpose::RomFile("pcsx2".to_string()),
                PathBuf::from("/roms/ps2/game.iso"),
            )
            .unwrap());

        while editor.rom_folders.len() < crate::backend::MAX_EMULATOR_ROM_FOLDERS {
            let path = format!("/roms/{}", editor.rom_folders.len());
            editor.add_rom_folder(&path).unwrap();
        }
        assert_eq!(
            editor.add_rom_folder("/roms/demais"),
            Err(RomFolderAddError::Limit)
        );
    }
}

// The renderer provides sRGB RGBA pixels in physical window dimensions.
fn blur_update_backdrop(shot: window::Screenshot) -> Option<Handle> {
    let (w, h) = (shot.size.width, shot.size.height);
    if w == 0 || h == 0 || w > 16384 || h > 16384 { return None; }
    let image = image::RgbaImage::from_raw(w, h, shot.rgba.to_vec())?;
    // Work on a quarter-resolution image to avoid blocking the UI or allocating
    // several full-resolution intermediate Gaussian buffers.
    let small = image::imageops::resize(&image, (w / 4).max(1), (h / 4).max(1), image::imageops::FilterType::Triangle);
    let blurred = image::imageops::blur(&small, 3.0);
    Some(Handle::from_rgba(blurred.width(), blurred.height(), blurred.into_raw()))
}

#[cfg(test)]
mod update_backdrop_tests {
    use super::*;

    #[test]
    fn gaussian_backdrop_blends_distinct_pixels() {
        let mut pixels = vec![0; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let i = (y * 32 + x) * 4;
                pixels[i] = if x < 16 { 0 } else { 255 };
                pixels[i + 3] = 255;
            }
        }
        let shot = window::Screenshot::new(pixels, iced::Size::new(32, 32), 1.0);
        let Handle::Rgba { pixels, width, height, .. } = blur_update_backdrop(shot).unwrap() else { panic!("expected RGBA") };
        assert_eq!((width, height), (8, 8));
        assert!(pixels[4 * (4 * 8 + 3)] > 0);
        assert!(pixels[4 * (4 * 8 + 4)] < 255);
    }

    #[test]
    fn invalid_backdrop_falls_back() {
        let shot = window::Screenshot::new(vec![0; 3], iced::Size::new(4, 4), 1.0);
        assert!(blur_update_backdrop(shot).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn steam_refresh_unlocks_launch_and_keeps_selection_and_cmd() {
        let root = std::env::temp_dir().join(format!(
            "arcadia-native-steam-refresh-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let backend = Backend::new(
            arcadia_ipc::AppState::new(root.clone()),
            tokio::runtime::Handle::current(),
        );
        let (mut app, boot) = App::boot(backend);
        drop(boot);

        let id = "steam:413150".to_string();
        let pending = Game::from_value(json!({
            "id": id,
            "title": "Stardew Valley",
            "launcher": "steam",
            "launch_cmd": ["steam", "steam://rungameid/413150"],
            "installed": false,
            "playtime_minutes": 120
        }))
        .expect("jogo");
        app.library.ready(vec![pending.clone()]);
        app.screen = Screen::Game(id.clone());

        drop(app.update(Message::LaunchGame(id.clone())));
        assert_eq!(app.launch.status, GameRunStatus::Idle);
        assert!(app.launch.error.is_some(), "Jogar abriu com installed:false");
        assert!(app.steam_install_pending());

        let mut installed = pending;
        installed.installed = true;
        drop(app.light_library_refresh());
        let epoch = app.library_epoch;
        drop(app.update(Message::LibraryRefreshed {
            epoch,
            result: Ok(vec![installed]),
        }));

        assert!(matches!(&app.screen, Screen::Game(current) if current == &id));
        let game = app.library.game(&id).expect("jogo continua na lista");
        assert!(game.installed, "refresh não aplicou o installed do IPC");
        assert_eq!(game.playtime_minutes, 120);
        assert!(!app.steam_install_pending(), "poll continuaria ligado");

        drop(app.update(Message::LaunchGame(id.clone())));
        assert!(app.launch.error.is_none());
        assert_eq!(app.launch.status, GameRunStatus::Launching);
        assert_eq!(app.launch.game_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            app.library.game(&id).unwrap().launch_cmd,
            vec!["steam".to_string(), "steam://rungameid/413150".to_string()]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn background_refresh_failure_keeps_ready_library() {
        let root = std::env::temp_dir().join(format!(
            "arcadia-native-steam-refresh-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let backend = Backend::new(
            arcadia_ipc::AppState::new(root.clone()),
            tokio::runtime::Handle::current(),
        );
        let (mut app, boot) = App::boot(backend);
        drop(boot);
        let id = "steam:413150".to_string();
        app.library.ready(vec![Game::from_value(json!({
            "id": id, "title": "Stardew Valley", "launcher": "steam", "installed": false
        }))
        .unwrap()]);
        app.screen = Screen::Game(id.clone());
        assert!(app.steam_install_pending());
        drop(app.light_library_refresh());
        let epoch = app.library_epoch;
        assert!(app.library_poll_in_flight);
        // Segunda chamada com requisição em voo não dispara outra leitura.
        drop(app.light_library_refresh());
        drop(app.update(Message::LibraryRefreshed {
            epoch,
            result: Err("ipc fora do ar".into()),
        }));
        assert!(!app.library_poll_in_flight);
        assert_eq!(app.library.status, LibraryStatus::Ready);
        assert_eq!(app.library.games.len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_refresh_cannot_resurrect_older_library_or_clear_new_flag() {
        let root = std::env::temp_dir().join(format!(
            "arcadia-native-steam-refresh-stale-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let backend = Backend::new(
            arcadia_ipc::AppState::new(root.clone()),
            tokio::runtime::Handle::current(),
        );
        let (mut app, boot) = App::boot(backend);
        drop(boot);

        let old = Game::from_value(json!({
            "id": "steam:413150",
            "title": "Stardew Valley",
            "launcher": "steam",
            "launch_cmd": ["steam", "steam://rungameid/413150"],
            "installed": false
        }))
        .expect("jogo");
        app.library.ready(vec![old.clone()]);

        drop(app.light_library_refresh());
        let stale_epoch = app.library_epoch;
        assert!(app.library_poll_in_flight);

        // Carga autoritativa mais nova substitui a biblioteca (vazia).
        drop(app.update(Message::LibraryLoaded(Ok(Vec::new()))));
        assert!(app.library.games.is_empty());
        assert!(!app.library_poll_in_flight);

        // A resposta velha não pode ressuscitar o jogo removido.
        drop(app.update(Message::LibraryRefreshed {
            epoch: stale_epoch,
            result: Ok(vec![old.clone()]),
        }));
        assert!(
            app.library.games.is_empty(),
            "resposta velha ressuscitou a biblioteca anterior"
        );

        // Um refresh novo tem o flag; erro velho não pode apagá-lo.
        drop(app.light_library_refresh());
        let new_epoch = app.library_epoch;
        assert_ne!(new_epoch, stale_epoch);
        assert!(app.library_poll_in_flight);
        drop(app.update(Message::LibraryRefreshed {
            epoch: stale_epoch,
            result: Err("leitura antiga".into()),
        }));
        assert!(
            app.library_poll_in_flight,
            "erro velho limpou o flag do refresh novo"
        );

        let mut fresh = old;
        fresh.installed = true;
        drop(app.update(Message::LibraryRefreshed {
            epoch: new_epoch,
            result: Ok(vec![fresh]),
        }));
        assert!(!app.library_poll_in_flight);
        assert!(app.library.game("steam:413150").unwrap().installed);

        let _ = std::fs::remove_dir_all(root);
    }
}
