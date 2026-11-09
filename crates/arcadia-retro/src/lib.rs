//! Núcleo retrô — parser de título, registro de sistemas e matcher de jogos.
//!
//! Port 1:1 dos módulos puros do cliente Electron (`app/electron/`):
//! `retro-title-parser.js`, `retro-systems.js` e `retro-matcher.js`.

pub mod matcher;
pub mod systems;
pub mod title;

pub use matcher::{
    calculate_match_score, calculate_match_score_json, extract_all_serials, find_best_match,
    find_best_match_json, find_best_match_json_with, find_best_match_with, fuzzy_match,
    get_match_quality, match_batch, match_batch_json, match_batch_json_with, match_batch_with,
    normalize_title, title_tokens, BatchMatch, BatchResult, BatchStats, BestMatch,
    ConfidenceBuckets, Hashes, MatchOptions, MatchQuality, MatchScore, RetroGame, RetroOffer,
    UnmatchedOffer, CONFIDENCE_THRESHOLD_AUTO, CONFIDENCE_THRESHOLD_PROBABLE,
};
pub use systems::{
    extract_serial, get_retroachievements_console_id, get_system, is_builtin_system, list_systems,
    normalize_serial, resolve_system, System, SYSTEMS,
};
pub use title::{
    classify_release_kind, extract_playstation_serial, fold_retro_diacritics, is_retro_alias_group,
    is_retro_group_tag, is_retro_metadata_tag, is_retro_metadata_token, is_retro_release_group_tag,
    normalize_retro_title, normalize_retro_title_with_max, parse_retro_title, retro_tag_words,
    strip_retro_alias_parentheticals, strip_retro_groups, ParsedRetroTitle, PlayStationSerial,
    MAX_RAW_TITLE, MAX_TITLE,
};

pub fn crate_ready() -> bool {
    true
}
