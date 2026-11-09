//! Amigos e comunidade — port de `app/electron/supabase/friends.js` +
//! `app/electron/supabase/community.js`.
//!
//! Duas fachadas independentes, sem nenhum acoplamento ao Tauri/IPC:
//!
//! - [`FriendsService`] — busca, pedidos, lista com cache de 60 s, conquistas
//!   do amigo (cache de 60 s/5 min), perfil e remoção;
//! - [`CommunityService`] — reviews e coleções em `/community/v1`, com cache
//!   por conta para leitura offline.
//!
//! Ambas recebem [`arcadia_api::ApiClient`] + [`arcadia_store::accounts::AccountScope`]
//! e devolvem `serde_json::Value` já no envelope exato que o renderer espera.
//! Falhas são [`SocialError`], com código estável; o IPC converte com
//! [`SocialError::to_friends_envelope`] ou
//! [`SocialError::to_community_envelope`].
//!
//! # O que o IPC deve mapear
//!
//! | Canal | Método | Envelope de sucesso |
//! |---|---|---|
//! | `friends:search` | [`FriendsService::search`] | `{ ok, results }` |
//! | `friends:send` | [`FriendsService::send`] | `{ ok }` |
//! | `friends:accept` | [`FriendsService::accept`] | `{ ok }` |
//! | `friends:cancel` | [`FriendsService::cancel`] | `{ ok }` |
//! | `friends:list` | [`FriendsService::list`] | `{ ok, data, deCache }` |
//! | `friends:achievements` | [`FriendsService::friend_achievements_cached`] | `{ ok, achievements, deCache? }` |
//! | `friends:profile` | [`FriendsService::friend_profile`] | payload da RPC |
//! | `friends:remove` | [`FriendsService::remove_friend`] | `{ ok }` |
//! | `community:reviews` | [`CommunityService::list_reviews`] | `{ ok, reviews, items, pagination[, offline] }` |
//! | `community:review:create/update` | [`CommunityService::create_review`]/[`CommunityService::update_review`] | `{ ok, review, data }` |
//! | `community:review:remove` | [`CommunityService::remove_review`] | `{ ok }` |
//! | `community:review:report` | [`CommunityService::report_review`] | `{ ok, report }` |
//! | `community:collections` | [`CommunityService::list_collections`] | `{ ok, collections, lists, items, pagination[, offline] }` |
//! | `community:collection:get` | [`CommunityService::get_collection`] | `{ ok, collection, data[, offline] }` |
//! | `community:collection:create/update` | [`CommunityService::create_collection`]/[`CommunityService::update_collection`] | `{ ok, collection, data }` |
//! | `community:collection:remove` | [`CommunityService::remove_collection`] | `{ ok }` |
//! | `community:collection:item:add/replace/remove` | [`CommunityService::add_collection_item`]/[`CommunityService::replace_collection_items`]/[`CommunityService::remove_collection_item`] | `{ ok, ...payload, collection?, data }` |
//! | `community:collection:report` | [`CommunityService::report_collection`] | `{ ok, report }` |
//!
//! `friends:list` com cache chama [`FriendsService::set_updated_listener`] em
//! background; o IPC deve reemitir o `data` recebido em `friends:changed`.
//!
//! # Fora de escopo
//!
//! **Realtime de amigos (`watchRequests`) não é implementado**: exigiria o
//! WebSocket do backend (`postgres_changes` em `friendships`). O canal
//! `friends:request` fica para a camada de transporte realtime; nenhuma
//! conexão WebSocket é aberta por este crate.

pub mod community;
pub mod error;
pub mod friends;

mod util;

pub use community::{
    CommunityService, Page, PageOptions, COMMUNITY_CACHE_FILE, COMMUNITY_CACHE_VERSION,
    DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE,
};
pub use error::SocialError;
pub use friends::{
    canonical_pair, CanonicalPair, FriendsService, FriendsUpdatedListener, FRIENDS_CACHE_FILE,
    FRIENDS_CACHE_TTL_MS, FRIEND_ACHIEVEMENTS_CACHE_FILE, FRIEND_ACHIEVEMENTS_TTL_MS,
};

/// Resultado da camada social.
pub type SocialResult<T> = std::result::Result<T, SocialError>;
