//! Steam: VDF, conta, horas jogadas e notícias.
//!
//! Porte 1:1 dos módulos congelados do cliente Electron:
//! `vdf.js`, `steam-account.js`, `steam-horas-servidor.js` e `steam-news.js`
//! (mais o handler `news:game` de `main.js`). O parser e os extratores são
//! funções puras: nenhum teste precisa de uma instalação da Steam nem toca na
//! rede. A consulta ao registro do Windows continua em
//! `arcadia-launch::steam_path`; [`paths`] recebe essas raízes por injeção e
//! monta a lista de candidatos do `steam-path.js` (discos, Program Files,
//! Flatpak) sem executar `reg query`.

pub mod account;
mod js;
pub mod news;
pub mod paths;
pub mod playtime;
#[cfg(test)]
mod util;
pub mod vdf;

pub use account::{
    account_id_to_steamid, active_from_accounts, active_from_roots, active_login_user,
    capture_status, config_accounts_from_path, config_accounts_from_root, config_vdf_path,
    link_for_active, link_json, local_config_path, login_users_from_path, login_users_from_root,
    login_users_path, motivo_pausa, next_account, parse_config_accounts, parse_link_json,
    previous_account, read_link_file, select_account, select_active_from_accounts,
    select_active_login_user, steamid_to_account_id, userdata_account_dir, AccountRef,
    AccountSelector, CaptureContext, CaptureStatus, ConfigAccounts, ForceWindow, Link,
    RootedAccount, SelectedLoginUser, SteamAccount, BASE_STEAM_ID, DEFAULT_COMPONENT,
    JANELA_FORCA_MS, VINCULO,
};
pub use news::{
    extract_steam_news_image, is_steam_news_url, normalize_steam_image_url, parse_game_news,
    parse_game_news_text, HttpPageFetcher, NewsClient, NewsError, NoPageFetcher, PageFetcher,
    PageFuture, SteamNewsImageResolver, SteamNewsItem, NEWS_COUNT, NEWS_ENDPOINT, NEWS_FEED,
    NEWS_IMAGE_TTL, NEWS_MAX_LENGTH, NEWS_PAGE_TIMEOUT, STEAM_NEWS_HOSTS,
};
pub use paths::{candidate_roots, find_steam_root, is_steam_root, SteamPathEnv, SteamPlatform};
pub use playtime::{
    appid_key, find_apps, itens_para_enviar, locais_de_contas, locais_por_conta, mesclar_horas,
    parse_local_config, parse_reading, parse_server_rows, playtime_from_root, playtime_map,
    playtime_map_from_root, reading_from_path, reading_from_root, reading_from_roots,
    readings_from_roots, somar_por_appid, LocalPlaytime, PlaytimeByAccount, PlaytimeMap,
    PlaytimeReading, ServerPlaytime, SteamPlaytimeItem,
};
pub use vdf::{
    parse as parse_vdf, parse_bytes as parse_vdf_bytes, serialize as serialize_vdf, VdfError,
    VdfObject, VdfValue,
};

pub fn crate_ready() -> bool {
    true
}
