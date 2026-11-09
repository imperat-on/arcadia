//! Conquistas: schemas da Steam, matching, carga, consolidação e decisões de
//! watcher — porte dos módulos congelados em
//! `app/electron/achievements/`.
//!
//! Módulos:
//! - [`steam_bin`]: KeyValues binário da Valve (`UserGameStatsSchema_*.bin` e
//!   bins de progresso) e `progress_map` (`block|bit -> epoch`).
//! - [`schema`]: normalização do schema em conquistas (`apiname`, nome,
//!   descrição, hashes de ícone, `hidden`, `permission`).
//! - [`r#match`]: apiname/apelido/título único, appid `steam:<id>` e seriais
//!   `ACObsidian_Ach_40 -> 40`.
//! - [`loader`]: leitura/gravação do `achievements.json`, índice `block|bit`,
//!   merge schema+progresso e preservação de campos desconhecidos.
//! - [`consolidar`]: fusão de itens duplicados da mesma conquista.
//! - [`cracked_watcher`]: parsers dos crackers, schema do emulador, caminhos,
//!   matching e scans com filesystem/relógio injetados (sem timers).
//! - [`uplay`]: UPC/voices38, IDs, runtime e schema.
//! - [`manager`]: máquina de estados pending/reconciled/revoked e fila com
//!   dedupe/earliest-wins.
//!
//! A crate é pura: não faz rede, não inicia watchers, não cria threads e não
//! dorme. O cliente real injeta `SystemFs`/`SystemClock` e chama os decisores.

pub mod consolidar;
pub mod cracked_watcher;
pub mod fsx;
mod js;
pub mod loader;
pub mod manager;
pub mod r#match;
pub mod schema;
pub mod steam_bin;
#[cfg(test)]
mod test_support;
pub mod uplay;

pub use consolidar::{
    apinames_de, consolidar_items, consolidar_items_with_report, fundir, ligados, normalizar,
};
pub use cracked_watcher::{
    apply_unlocks, crack_candidates, crack_roots, detect_crack_group, find_emulator_schema,
    find_item_for_unlock, library_games, numeric_uplay_dirs, parse_codex, parse_crack_text,
    parse_cream_api, parse_emulator_schema, parse_flt_file_names, parse_goldberg,
    parse_goldberg_json, parse_online_fix, parse_razor1911, parse_rld, parse_skidrow,
    parse_user_stats, payload_for_unlock, reconcile_upc, resolve_exe_dir, resolve_prefix,
    settings_for_entry, uplay_ids_for_scan, ApplyResult, CrackCandidate, CrackEnvironment,
    CrackGroup, CrackRoots, CrackScanReport, CrackedWatcher, DetectedUnlock, EmulatorSchemaFile,
    EmulatorTitles, IniFile, IniSection, Revocation, ScanGameOptions, UnlockPayload,
    IDIOMAS_SCHEMA, INTERVALO_POLL, ROOT_SECTION,
};
pub use fsx::{Clock, Platform, SystemClock, SystemFs, WatcherFs};
pub use loader::{
    collect_appids, configured_appids, copy_icon_to_cache, first_text, load_achievements,
    load_all_schemas, load_item_index, load_item_index_from_path, resolve_schema_for_appid,
    save_achievements, schema_item_view, sort_items, steam_icon_url, AchievementsStore, CdnIcons,
    IconResolver, ItemIndex, LoadOptions, LoadReport, NoScrapeProvider, RebuildReport,
    ResolvedAppSchema, SchemaOrigin, ScrapeSchemaProvider, SteamIcons, SyncRecord, STEAM_ICON_HOST,
};
pub use manager::{
    has_steam_bin_for, AchievementManager, AchievementRecord, AchievementState, Observation,
    QueueRecord, SteamBinDecision, SteamBinWatcher, SteamUnlockPayload, SyncQueue,
};
pub use r#match::{
    aliases_of, build_index, extract_appid, extract_appid_value, find_by_serial, find_item,
    find_item_index, is_plain_object, item_serial_id, normalize_appid, normalize_numeric_id,
    normalize_title, normalize_title_value, numeric_achievement_id, numeric_achievement_id_value,
    remember_alias, resolve_appid, AchievementIndex,
};
pub use schema::{
    appid_from_schema_filename, list_schema_files, load_schema_catalog, load_schema_for_appid,
    parse_progress_filename, pick_localized, schema_filename, schema_from_bytes, schema_from_kv,
    schema_from_saved_items, schema_from_scrape, schema_path, AchievementSchema, SchemaAchievement,
    LOCALE_CHAIN,
};
pub use steam_bin::{
    block_bit_key, json_block_bit_key, load_binary_file, load_kv_bin, load_progress_map,
    parse_binary, parse_binary_object, progress_map, progress_map_bytes, read_kv, KvError,
    KvObject, KvValue, KV_FLOAT32, KV_INT32, KV_INT64, KV_OBJECT, KV_OBJECT_END, KV_STRING,
    KV_UINT64, MAX_DEPTH,
};
pub use uplay::{
    build_uplay_schema, earned_value, epoch_milliseconds, ini_setting, inspect_uplay_installation,
    item_uplay_id, known_upc_id, numeric_achievement_id_str, parse_upc, resolve_uplay_id,
    set_ini_setting, upc_runtime_map, uplay_runtime_path, uplay_save_root, uplay_schema_path,
    validate_uplay_schema, UpcRuntime, UpcUnlock, UplayInstallation, UplaySchemaError,
    UplayValidateOptions, UplayValidation, KNOWN_UPC_IDS, UPLAY_SAVE_DIR, USUARIO_WINE,
};

pub fn crate_ready() -> bool {
    true
}
