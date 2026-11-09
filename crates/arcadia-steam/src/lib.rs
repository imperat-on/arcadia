//! Steam: VDF, conta, horas jogadas e notícias.
//!
//! Porte 1:1 dos módulos congelados do cliente Electron:
//! `vdf.js`, `steam-account.js`, `steam-horas-servidor.js` e `steam-news.js`
//! (mais o handler `news:game` de `main.js`). O parser e os extratores são
//! funções puras: nenhum teste precisa de uma instalação da Steam nem toca na
//! rede. A detecção de onde a Steam está instalada (registro do Windows,
//! discos, Flatpak) continua em `arcadia-launch::steam_path`; aqui só existem
//! construtores de caminho a partir de uma raiz já resolvida.

pub mod account;
mod js;
pub mod news;
pub mod paths;
pub mod playtime;
pub mod util;
pub mod vdf;

pub use account::{
    account_id_to_steamid, active_from_accounts, active_login_user, capture_status,
    config_accounts_from_root, config_vdf_path, link_for_active, link_json, local_config_path,
    login_users_from_root, login_users_path, motivo_pausa, next_account, parse_config_accounts,
    parse_link_json, previous_account, select_account, select_active_from_accounts,
    select_active_login_user, steamid_to_account_id, userdata_account_dir, AccountRef,
    AccountSelector, CaptureContext, CaptureStatus, ConfigAccounts, ForceWindow, Link,
    SelectedLoginUser, SteamAccount, BASE_STEAM_ID, DEFAULT_COMPONENT, JANELA_FORCA_MS, VINCULO,
};
pub use news::{
    extract_steam_news_image, is_steam_news_url, normalize_steam_image_url, parse_game_news,
    parse_game_news_text, HttpPageFetcher, NewsClient, NewsError, NoPageFetcher, PageFetcher,
    PageFuture, SteamNewsImageResolver, SteamNewsItem, NEWS_COUNT, NEWS_ENDPOINT, NEWS_FEED,
    NEWS_IMAGE_TTL, NEWS_MAX_LENGTH, NEWS_PAGE_TIMEOUT, STEAM_NEWS_HOSTS,
};
pub use playtime::{
    appid_key, find_apps, itens_para_enviar, locais_de_contas, locais_por_conta, mesclar_horas,
    parse_local_config, parse_reading, parse_server_rows, playtime_from_root, playtime_map,
    playtime_map_from_root, reading_from_root, somar_por_appid, LocalPlaytime, PlaytimeByAccount,
    PlaytimeMap, PlaytimeReading, ServerPlaytime, SteamPlaytimeItem,
};
pub use vdf::{
    parse as parse_vdf, parse_bytes as parse_vdf_bytes, serialize as serialize_vdf, VdfError,
    VdfObject, VdfValue,
};

pub fn crate_ready() -> bool {
    true
}
