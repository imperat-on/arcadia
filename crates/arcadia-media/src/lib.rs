//! Metadados e mídia do Arcadia.
//!
//! Port da camada de provedores do cliente Electron congelado
//! (`app/electron/metadata.js`, `hltb.js` e `trailer-service.js`):
//!
//! - [`providers`]: SteamGridDB, Steam (CDN + loja), IGDB (proxy Playnite),
//!   Xbox/Microsoft Store, PlayStation Store e Wallhaven;
//! - [`art`]: arte 1:1 (`arteQuadrada`), logo automática, `urlCardDe`,
//!   download e nomes de arquivo de `art/`;
//! - [`hltb`]: busca/parse do HowLongToBeat com cache de 30 dias;
//! - [`trailer`]: construção dos comandos do `yt-dlp` (sem spawn).
//!
//! Nenhum teste sai para a internet: [`http::Fetcher`] é injetado e os
//! contratos HTTP usam `wiremock` em loopback.
//!
//! ## Fora do escopo (documentado)
//!
//! - O processo Electron/Tauri que executa `yt-dlp` e persiste progresso;
//! - escrita do cache da IGDB (o LRU é só memória, como no JS);
//! - `local-search.js`: índice local de catálogo/biblioteca não é provedor de
//!   metadados e ficou fora desta camada.
//!
//! ## Chaves
//!
//! A chave da SteamGridDB vem de [`providers::steamgriddb::SteamGridDb`], que
//! o chamador constrói com o valor da configuração. Nenhuma chave é constante
//! neste crate. O proxy da IGDB é público e não usa chave.

pub mod art;
pub mod cache;
pub mod error;
pub mod hltb;
pub mod http;
pub mod providers;
pub mod text;
pub mod trailer;

#[cfg(test)]
mod test_support;

pub use art::{
    art_cache_base, art_dir, art_pick_path, art_to_delete, art_to_delete_in, download_path,
    download_to, download_to_with, logo_psn, safe_id, square_art_psn, square_art_sgdb,
    square_art_worker, square_art_xbox, url_card_de, ArtResolver, ArtWriter, DownloadedArt,
    FsWriter, LogoArt, SquareArt, ART_DIR_NAME,
};
pub use cache::{BreakerState, CircuitBreaker, Clock, LruCache, ManualClock, SystemClock};
pub use error::{FetchError, MediaError, Result};
pub use hltb::{
    discover_hash, discover_hash_with_base, normalize_hltb_title, seek, seek_body, seek_with_base,
    HltbClient, HltbTimes, HLTB_CACHE_TTL_MS, HLTB_UA,
};
pub use http::{
    encode_uri_component, get_json, get_text, query_string, Fetcher, HttpClientFetcher, Method,
    Request, Response,
};
pub use providers::{
    igdb, psn, steam, steamgriddb, wallhaven, xbox, ArtKind, Artwork, TextCandidate,
};
pub use providers::{
    igdb::{IgdbCandidate, IgdbClient, IgdbGame, IgdbImage, IgdbSearch, IGDB_UA},
    psn::{PsnClient, PsnGame, PsnTile},
    steam::{SteamClient, STEAM_FILES},
    steamgriddb::{SgdbArtOptions, SgdbGame, SteamGridDb},
    wallhaven::WallhavenClient,
    xbox::{XboxClient, XboxGame, XboxImage, XboxLocalizedProduct},
};
pub use text::{extension_from_url, normalize_title, strip_html, titles_match};
pub use trailer::{
    cookie_args, cookie_args_with, download_args, download_args_with_cookies, download_url_args,
    download_url_args_with_cookies, files_to_clear, is_available, local_path, local_path_with,
    parse_search_stdout, parse_stream_result, progress_events, safe_name, search_args,
    search_error_message, search_query, stream_url_args, stream_url_args_with_cookies,
    youtube_search_url, yt_search_spec, ProgressEvent, TrailerOptions, TrailerResult,
    TRAILER_EXTENSIONS, VIDEO_FORMAT,
};

pub fn crate_ready() -> bool {
    true
}
