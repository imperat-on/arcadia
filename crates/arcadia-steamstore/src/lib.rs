//! Loja Steam: busca, detalhes, install info, depots e biblioteca.
//!
//! Port 1:1 dos módulos congelados `app/electron/steamstore.js`,
//! `app/electron/steam-path.js` e das partes de loja de `main.js`.
//!
//! # Operações
//!
//! **Puras / cliente HTTP injetado (este crate):**
//! - [`store::StoreClient`]: `store:search`, `store:suggest`, `store:recent`,
//!   `store:warm`, `store:validarChaveHubcap`, `itensDaLoja`, `appdetails`
//!   (`game:sysinfo`).
//! - [`depot::DepotClient`]: `store:installInfo` (parse/merge de `.lua`,
//!   PICS/appinfo, enriquecimento de depots, picker, tamanhos).
//! - [`library`]: `store:installDir`, `store:libraries`, `store:addToSteam`,
//!   `store:removeFromSteam`, `store:removeDownloaded`, `store:status` (parte
//!   local), ACF/`libraryfolders`, detecção de downloads do Arcadia, stubs
//!   `store:addToLibrary`/`store:removeFromLibrary` (parte local).
//! - [`update`]: checagem de atualização de apps Steam instalados.
//!
//! **Precisam da borda (executor/Tauri; não fazem parte deste crate):**
//! - `store:install` (spawn do DepotDownloader), `store:ensureDotnet`,
//!   `store:ensureDepotDownloader`, `slssteam:launchSteam` e o restart da
//!   Steam;
//! - download/extração dos zips de manifesto (o crate só mescla o conteúdo já
//!   extraído) — nenhum download real acontece aqui;
//! - `store:status` nos campos que exigem processos (`dotnet --list-runtimes`,
//!   `depotdownloader`, plugin SLSsteam);
//! - `store:addToSteam`/`store:removeFromSteam` também têm, em `main.js`,
//!   efeitos na coleção da conta (`pending_games.json`, `owned`, overrides e
//!   push) que continuam no IPC/executor; este crate cobre a parte de disco da
//!   Steam/SLSsteam.
//!
//! Nenhum teste toca a rede real: [`arcadia_catalog::http::Fetcher`] é
//! injetado; as mutações de biblioteca usam um sistema de arquivos injetado
//! ([`library::LibraryFs`]).

pub mod depot;
pub mod library;
pub mod store;
#[cfg(test)]
mod test_support;
pub mod update;

pub use depot::{
    apply_branch_manifests, depot_picker_json, enrich_depots, essential_selection,
    install_dir_default, install_info_from_merged, latest_build_from_app_info,
    merge_provider_manifests, parse_app_info, parse_lua_files, parse_lua_manifest,
    preferred_language, sanitize_install_dir, selected_size, size_gib, AppInfo, BranchInfo,
    DepotClient, DepotGroup, DepotGroupKind, DepotInfo, InstallInfo, InstallInfoResponse, LuaDepot,
    LuaManifest, ManifestBranch, MergedManifest, ProviderManifest,
};
pub use library::{
    add_pending_stub, add_to_steam, appids_injected, appids_injected_from, arcadia_downloaded,
    discover_libraries, find_steam_base, game_install_dir, game_install_dir_or,
    installed_manifests, mark_steam_installed, parse_acf, parse_library_folders, pending_stub,
    read_pending_games, register_slssteam, remove_app_from_config, remove_downloaded,
    remove_from_steam, remove_pending_stub, sanitize_install_dir as sanitize_library_install_dir,
    scan_manifests, steam_apps_dirs, steam_base_candidates, steam_libraries, store_status,
    write_acf, write_acf_at, write_pending_games, AcfManifest, AddToSteamResponse, DownloadedApp,
    GameRef, LibraryFolder, LibraryFs, PendingUpdate, RemoveResult, SteamLibrary, StoreStatus,
    StoreStatusInputs, SystemFs,
};
pub use store::{
    extract_appid, format_price, is_accessory, normalize_appid, relevance, sort_games,
    steam_language, strip_html, valid_appid, AppDetails, AppDetailsResponse, AppMovie,
    AppScreenshot, HubcapKeyValidation, RecentResponse, SearchOptions, SearchResponse, StoreBases,
    StoreCacheSnapshot, StoreClient, StoreConfig, StoreError, StoreGame, StoreItem,
    StoreItemsResult, SuggestResponse, GAME_TYPE, GENRE_TTL_MS, HUBCAP_BASE, ITEMS_BATCH,
    ITEMS_MAX, ITEMS_TTL_MS, MANIFEST_TTL_MS, POPULAR_TTL_MS, RYUU_BASE, SEARCH_MAX, SEARCH_TTL_MS,
    STEAMSPY_API, STEAM_APPDETAILS, STEAM_CDN, STEAM_CDN_AKAMAI, STEAM_ITEMS, STEAM_ITEMS_ASSETS,
    STEAM_LANG_MAP, STEAM_SEARCH, SUGGEST_LIMIT, SUSHI_BASE, SUSHI_TREE, SUSHI_TTL_MS,
    TWENTYTWO_BASE, USER_AGENT,
};
pub use update::{
    build_update, buildid_is_newer, buildid_of, check_updates, latest_from_app_info,
    state_flags_status, update_report, CheckUpdatesOptions, LatestBuild, SteamUpdate, UpdateReport,
    UpdateStatus, STATE_ADDING_FILES, STATE_BACKUP_RUNNING, STATE_COMMITTING, STATE_DOWNLOADING,
    STATE_FILES_CORRUPT, STATE_FILES_MISSING, STATE_FULLY_INSTALLED, STATE_PREALLOCATING,
    STATE_RECONFIGURING, STATE_SHARED_ONLY, STATE_STAGING, STATE_UNINSTALLED, STATE_UNINSTALLING,
    STATE_UPDATE_OPTIONAL, STATE_UPDATE_PAUSED, STATE_UPDATE_QUEUED, STATE_UPDATE_REQUIRED,
    STATE_UPDATE_RUNNING, STATE_UPDATE_STARTED, STATE_UPDATE_STOPPING, STATE_VALIDATING,
};

pub fn crate_ready() -> bool {
    true
}
