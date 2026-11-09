//! Catálogo remoto, hosters/debrid e registro de fontes locais.
//!
//! Port 1:1 de `app/electron/catalog.js`, `hosters.js` e `sources.js` (mais os
//! helpers de payload de `app/electron/supabase/sources.js`) sobre o cliente
//! HTTP compartilhado do [`arcadia_api`].
//!
//! - [`catalog`]: `catalogGet` com espelho em disco, ETag/304, fallback
//!   offline, timeout curto quando há espelho e dedupe em voo;
//! - [`hosters`]: resolvedores gratuitos + Real-Debrid, TorBox, AllDebrid e
//!   Premiumize (link de página e magnet);
//! - [`sources`]: registro, cache, índice leve, busca ranqueada, `getGame` e
//!   payloads `push_sources`/`pull_sources`.
//!
//! Não há rede real nos testes: [`http::Fetcher`] é injetado e os contratos
//! usam `wiremock` local. Nenhum token/chave é logado (`Debug` redigido).

pub mod catalog;
pub mod hosters;
pub mod http;
pub mod sources;

pub use catalog::{
    catalog_mirror_path, mirror_file_name, AuthProvider, CatalogClient, CatalogError,
    CatalogOptions, CatalogResponse, FsMirrorStore, MemoryMirrorStore, MirrorStore, NoAuth,
    StaticAuth, CATALOG_PREFIX, ESPELHO_DIR_NAME, FULL_TIMEOUT, MIRROR_TIMEOUT,
};
pub use hosters::{
    has_debrid_configured, mask_token, AbortSignal, AtomicAbort, DebridClient, DebridEndpoints,
    DebridError, DebridProvider, DebridStatus, DebridTokens, NeverAbort, NoopSleeper, PollWindow,
    ResolvedLink, Sleeper, TokioSleeper, ARCADIA_UA, BROWSER_UA, MAGNET_INTERVAL, MAGNET_TIMEOUT,
    TORBOX_WEB_INTERVAL, TORBOX_WEB_TIMEOUT,
};
pub use http::{
    encode_uri_component, form_urlencode, FetchError, Fetcher, HttpClientFetcher, Method, Request,
    Response,
};
pub use sources::{
    buscar_no_indice, canonico_folded, fold_titulo, match_titulo, match_titulo_with,
    next_watermarks, palavras_titulo, pull_sources, push_payload, source_id, titulo_canonico,
    valid_source_id, valid_source_url, validar_downloads, AddSourceResult, CanonicalTitle,
    ChallengeSolver, Clock, DownloadOutcome, FsSourceStorage, IndexEntry, ManualClock,
    MemorySourceStorage, NoChallenge, PullOutcome, RemoveSourceResult, SearchResult, SourceCatalog,
    SourceGameResult, SourceInfo, SourceRegistry, SourceStorage, SourceSyncItem, SourceSyncResult,
    SystemClock, TitleMatch, CACHE_DIR, DEFAULT_LIMIT, INDEX_FILE, REGISTRY_FILE, SOURCE_TIMEOUT,
};
