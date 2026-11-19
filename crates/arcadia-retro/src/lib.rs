//! Núcleo retrô — parser de título, registro de sistemas, matcher, catálogo,
//! ofertas, repositório local e metadados.
//!
//! Port 1:1 dos módulos puros do cliente Electron (`app/electron/`):
//! `retro-title-parser.js`, `retro-systems.js`, `retro-matcher.js`,
//! `retro-catalog.js`, `retro-catalog-builder.js`, `retro-catalog-v2.js`,
//! `retro-server-catalog.js`, `retro-offers.js`, `retro-repository.js`,
//! `retro-metadata-igdb.js` e `retro-metadata-enricher.js`.
//!
//! Nenhum teste toca a rede: catálogo e IGDB recebem um `Fetcher` injetado;
//! repositório e cache usam diretórios temporários.

pub mod catalog;
pub mod matcher;
pub mod metadata;
pub mod offers;
pub mod repository;
pub mod systems;
pub mod title;
mod util;

pub use catalog::{
    apply_overrides_to_catalog, build_canonical_catalog, clean_retro_title, create_canonical_game,
    create_retro_catalog, create_retro_catalog_v2, generate_game_id, merge_offers, normalize_game,
    normalize_payload, normalize_source, normalize_uri, read_cache, read_local_source_payload,
    rebuild_catalog_from_sources, safe_public_url_str, safe_public_url_value, source_cache_path,
    source_id, write_cache, BuildOptions, CanonicalGame, Catalog, CatalogBuild, CatalogError,
    CatalogExport, CatalogGame, CatalogSource, CatalogStats, CatalogUnmatched, Identity,
    MatchQualityStats, MergedOffers, MigrationResult, PersonalCandidate, PersonalGame,
    PersonalSources, RefreshResult, RemoteCatalogProvider, RetroCatalog, RetroCatalogV2,
    RetroServerCatalog, V1Catalog, CACHE_FILE_NAME, CACHE_TTL_MS, CACHE_VERSION, HYDRA_API,
    MAX_DESCRIPTION, MAX_GAMES, MAX_JSON_BYTES, MAX_LIMIT, MAX_QUERY, MAX_SOURCES, MAX_URIS,
};
pub use matcher::{
    calculate_match_score, calculate_match_score_json, extract_all_serials, find_best_match,
    find_best_match_json, find_best_match_json_with, find_best_match_with, fuzzy_match,
    get_match_quality, match_batch, match_batch_json, match_batch_json_with, match_batch_with,
    normalize_title, title_tokens, BatchMatch, BatchResult, BatchStats, BestMatch,
    ConfidenceBuckets, Hashes, MatchOptions, MatchQuality, MatchScore, RetroGame, RetroOffer,
    UnmatchedOffer, CONFIDENCE_THRESHOLD_AUTO, CONFIDENCE_THRESHOLD_PROBABLE,
};
pub use metadata::{
    build_serial_query, build_title_query, create_metadata_enricher, get_credentials_hash,
    igdb_platform_id, merge_unique_values, normalize_igdb_game, CacheStats, EnrichOptions,
    EnrichProgress, IgdbArtwork, IgdbClient, IgdbCredentials, IgdbLookup, IgdbMetadata,
    ManifestEntry, MetadataEnricher, MetadataManifest, ARTWORK_TTL_DAYS,
    MAX_CACHE_SIZE_MB as METADATA_MAX_CACHE_SIZE_MB, METADATA_CACHE_VERSION, METADATA_TTL_DAYS,
    METADATA_TTL_MS, PLATFORM_MAP, RATE_LIMIT_DELAY_MS,
};
pub use offers::{
    classify_release_kind as classify_offer_release_kind, create_offer_summary, deserialize_offers,
    extract_languages, extract_region, filter_by_release_kind, generate_offer_fingerprint,
    group_offers_by_game, normalize_offer, process_source_downloads, serialize_offers,
    update_offer_match, Offer, OfferMatch, OfferSource,
};
pub use repository::{
    create_retro_repository, ArtworkFile, CatalogIndex, LibraryResult, MatchesFile, OffersFile,
    OverridesFile, RepositoryError, RepositoryManifest, RepositoryStats, RetroLibrary,
    RetroRepository, UnmatchedFile, CACHE_VERSION as REPOSITORY_CACHE_VERSION,
    MAX_CACHE_SIZE_MB as REPOSITORY_MAX_CACHE_SIZE_MB, MAX_OFFERS,
};
pub use systems::{
    extract_serial, get_retroachievements_console_id, get_system, is_builtin_system, list_systems,
    normalize_serial, resolve_system, System, SYSTEMS,
};
pub use title::{
    classify_release_kind, clean_text, extract_playstation_serial, fold_retro_diacritics,
    is_retro_alias_group, is_retro_group_tag, is_retro_metadata_tag, is_retro_metadata_token,
    is_retro_release_group_tag, normalize_retro_title, normalize_retro_title_with_max,
    parse_retro_title, retro_tag_words, strip_retro_alias_parentheticals, strip_retro_groups,
    ParsedRetroTitle, PlayStationSerial, MAX_RAW_TITLE, MAX_TITLE,
};

pub fn crate_ready() -> bool {
    true
}
