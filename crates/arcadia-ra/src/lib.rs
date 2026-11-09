//! Cliente RetroAchievements — port 1:1 de
//! `app/electron/retroachievements/client.js` + os handlers
//! `retroachievements:*` de `app/electron/main.js`.
//!
//! # Módulos
//!
//! - [`client`] — HTTP puro: login (`dorequest.php`), validação da Web API Key
//!   (`API_GetUserSummary.php`), busca de jogo (`API_GetGameList.php`) e
//!   progresso (`API_GetGameInfoAndUserProgress.php`);
//! - [`progress`] — normalização de achievements (badges, datas ISO, contadores
//!   e percentuais) e resolução de console via [`arcadia_retro`];
//! - [`session`] — estado usuário/token/Web API Key com armazenamento de
//!   config injetado (sem IPC e sem tocar `config.json`).
//!
//! # Garantias
//!
//! - a senha só existe como argumento emprestado de [`client::RetroAchievementsClient::login`];
//! - o que é persistido é o token de sessão, nunca a senha nem a Web API Key no
//!   lugar do token;
//! - nenhum teste abre rede externa: todos usam um servidor HTTP local
//!   (`wiremock`).
//!
//! # O que o IPC deve mapear
//!
//! | Canal | Método | Envelope de sucesso |
//! |---|---|---|
//! | `retroachievements:login` | [`session::RaSession::login`] | `{ ok: true, username }` |
//! | `retroachievements:logout` | [`session::RaSession::logout`] | `{ ok: true }` |
//! | `retroachievements:status` | [`session::RaSession::status`] | `{ ok: true, connected, username }` |
//! | `retroachievements:setApiKey` | [`session::RaSession::set_api_key`] | `{ ok: true }` |
//! | `retroachievements:gameProgress` | [`session::RaSession::game_progress`] | `{ ok: true, game, achievements }` ou `{ ok: true, game: null, achievements: [] }` |
//!
//! `retroachievements:applyToEmulator` continua fora deste crate: use
//! [`session::RaSession::credentials`] + `arcadia_emulators::configure_emulator_credentials`.

pub mod client;
pub mod error;
pub mod progress;
pub mod session;
mod util;

pub use client::{
    GameRef, LoginResult, RetroAchievementsClient, RetroAchievementsClientBuilder, VerifiedUser,
    DEFAULT_HOST, GAME_LIST_PATH, GAME_PROGRESS_PATH, LOGIN_PATH, MEDIA_HOST, REQUEST_TIMEOUT,
    USER_AGENT, USER_SUMMARY_PATH,
};
pub use error::{
    RaError, RaResult, CODE_CHAVE_INVALIDA, CODE_CREDENCIAIS_INVALIDAS, CODE_CREDENCIAIS_VAZIAS,
    CODE_FALHA_BUSCA, CODE_FALHA_REDE, CODE_NAO_CONECTADO, CODE_PARAMETROS_INVALIDOS,
    CODE_RESPOSTA_INVALIDA, CODE_SEM_WEB_API_KEY, CODE_SISTEMA_NAO_SUPORTADO, CODE_TEMPO_ESGOTADO,
};
pub use progress::{
    badge_url, completion_percentage, console_id_for_system, find_game_for_system,
    normalize_earned_date, parse_game_progress, resolve_game_id, Achievement, GameInfo,
    GameProgress,
};
pub use session::{
    MemoryConfigStorage, RaConfigStorage, RaCredentials, RaSession, RaStatus, KEY_TOKEN,
    KEY_USERNAME, KEY_WEB_API_KEY,
};

pub fn crate_ready() -> bool {
    true
}
