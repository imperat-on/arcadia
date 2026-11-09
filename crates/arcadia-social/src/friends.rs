//! Amigos — port 1:1 de `app/electron/supabase/friends.js` (MAIN process).
//!
//! Modelo do servidor: uma linha canônica por par (`user_a < user_b`) com
//! `requester_id` + `status`. As operações usam exatamente as mesmas tabelas,
//! RPCs e payloads do cliente Electron:
//!
//! | Operação | Chamada |
//! |---|---|
//! | busca | `GET /rest/v1/profiles?select=...&username=ilike.<q>%&limit=20` |
//! | relações da busca | `GET /rest/v1/friendships?select=...&or=(...)` |
//! | enviar | `POST /rest/v1/friendships` |
//! | aceitar | `PATCH /rest/v1/friendships?...&status=eq.pending&requester_id=eq.<id>` |
//! | cancelar | `DELETE /rest/v1/friendships?...&status=eq.pending&requester_id=eq.<eu>` |
//! | listar | `GET /rest/v1/friendships` com `pa`/`pb` embutidos |
//! | conquistas | `POST /rest/v1/rpc/friend_achievements` (`p_friend`) |
//! | perfil | `POST /rest/v1/rpc/friend_profile` (`p_friend`) |
//! | remover | `DELETE /rest/v1/friendships?...&status=eq.accepted` |
//!
//! Cache local por conta (mesmo arquivo do Electron):
//! `friends_cache.json` (`{ ts, data }`) com TTL de 60 s e
//! `friend_achievements_cache.json` (`{ "<friendId>": { ts, data } }`) com TTL
//! de 60 s na leitura direta e 5 min no caminho `friend_achievements_cached`
//! usado pelo IPC. Escritas são atômicas (tmp + rename) e campos desconhecidos
//! do arquivo são preservados.
//!
//! **Realtime (`watchRequests`) está fora do escopo desta camada.** O canal
//! `friends:request` depende de WebSocket (`postgres_changes` na tabela
//! `friendships`), que não é implementado aqui. O evento `friends:changed` é
//! exposto via [`FriendsService::set_updated_listener`]: ao bater no cache,
//! `list` devolve o cache na hora e dispara um refresh em background; quando o
//! refresh termina, o listener recebe o novo `data` (o IPC deve reemitir em
//! `friends:changed`).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{json, Value};

use arcadia_api::ApiClient;
use arcadia_store::accounts::AccountScope;

use crate::error::{map_api_error, SocialError};
use crate::util;

pub const FRIENDS_CACHE_FILE: &str = "friends_cache.json";
pub const FRIEND_ACHIEVEMENTS_CACHE_FILE: &str = "friend_achievements_cache.json";
pub const FRIENDS_CACHE_TTL_MS: i64 = 60_000;
pub const FRIEND_ACHIEVEMENTS_TTL_MS: i64 = 5 * 60_000;

/// `select` exato de `buscarFresco` (inclui os recursos embutidos `pa`/`pb`).
const FRIENDS_SELECT: &str = "user_a, user_b, status, requester_id, created_at,\n       pa:profiles!friendships_user_a_fkey(id, username, avatar_url, display_name),\n       pb:profiles!friendships_user_b_fkey(id, username, avatar_url, display_name)";

/// Par canônico `(user_a, user_b)` com `user_a < user_b` (ordem UTF-16, a
/// mesma do `<` do JavaScript).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CanonicalPair {
    pub user_a: String,
    pub user_b: String,
}

/// Ordena os ids para a chave anti-duplicata do modelo `friendships`.
pub fn canonical_pair(a: &str, b: &str) -> CanonicalPair {
    if js_less_than(a, b) {
        CanonicalPair {
            user_a: a.to_string(),
            user_b: b.to_string(),
        }
    } else {
        CanonicalPair {
            user_a: b.to_string(),
            user_b: a.to_string(),
        }
    }
}

fn js_less_than(left: &str, right: &str) -> bool {
    left.encode_utf16().cmp(right.encode_utf16()).is_lt()
}

/// Callback de `friends:changed` (recebe o `data` novo).
pub type FriendsUpdatedListener = Arc<dyn Fn(&Value) + Send + Sync>;

/// Fachada de amigos. `Clone` compartilha o cliente, o escopo e o listener.
#[derive(Clone)]
pub struct FriendsService {
    client: ApiClient,
    scope: Arc<Mutex<AccountScope>>,
    on_updated: Option<FriendsUpdatedListener>,
}

impl FriendsService {
    pub fn new(client: ApiClient, scope: AccountScope) -> Self {
        Self {
            client,
            scope: Arc::new(Mutex::new(scope)),
            on_updated: None,
        }
    }

    /// Registra o listener do refresh em background (`friends:changed`).
    pub fn set_updated_listener(&mut self, listener: impl Fn(&Value) + Send + Sync + 'static) {
        self.on_updated = Some(Arc::new(listener));
    }

    pub fn with_updated_listener(
        mut self,
        listener: impl Fn(&Value) + Send + Sync + 'static,
    ) -> Self {
        self.set_updated_listener(listener);
        self
    }

    pub fn clear_updated_listener(&mut self) {
        self.on_updated = None;
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    /// Troca a conta ativa (None = guest). O IPC chama isto no evento de auth
    /// para que caches e futuras requisições usem o escopo correto.
    pub fn set_active(&self, username: Option<&str>) -> io::Result<()> {
        self.lock_scope().set_active(username)
    }

    pub fn account(&self) -> Option<String> {
        self.lock_scope().active().map(str::to_string)
    }

    fn lock_scope(&self) -> std::sync::MutexGuard<'_, AccountScope> {
        self.scope
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn cache_path(&self) -> PathBuf {
        self.lock_scope().account_file(FRIENDS_CACHE_FILE)
    }

    pub fn achievements_cache_path(&self) -> PathBuf {
        self.lock_scope()
            .account_file(FRIEND_ACHIEVEMENTS_CACHE_FILE)
    }

    /// Busca usuários por prefixo do username, exclui o próprio usuário e
    /// marca a relação existente (`status`/`incoming`), como `search()`.
    ///
    /// Envelope: `{ ok: true, results: [...] }`.
    pub async fn search(&self, query: &str) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        let q = util::js_trim(query).to_lowercase();
        if q.is_empty() {
            return Ok(json!({ "ok": true, "results": [] }));
        }

        let profiles = self
            .client
            .from("profiles")
            .select("id, username, avatar_url, display_name")
            .ilike("username", format!("{q}%"))
            .limit(20)
            .await
            .map_err(map_api_error)?;
        let profiles: Vec<Value> = profiles.as_array().cloned().unwrap_or_default();
        let encontrados: Vec<&Value> = profiles
            .iter()
            .filter(|profile| profile.get("id").and_then(Value::as_str) != Some(me.as_str()))
            .collect();
        if encontrados.is_empty() {
            return Ok(json!({ "ok": true, "results": [] }));
        }

        // Relações existentes comigo; falha aqui é ignorada (como no JS).
        let relation_condition = format!("user_a.eq.{me},user_b.eq.{me}");
        let relations = self
            .client
            .from("friendships")
            .select("user_a, user_b, status, requester_id")
            .or(relation_condition)
            .await
            .unwrap_or(Value::Null);
        let mut rel_by_user: HashMap<String, (Value, Value)> = HashMap::new();
        for relation in relations.as_array().into_iter().flatten() {
            let is_user_a_me = matches!(
                relation.get("user_a"),
                Some(Value::String(value)) if value == &me
            );
            let other = if is_user_a_me {
                relation.get("user_b")
            } else {
                relation.get("user_a")
            };
            rel_by_user.insert(
                util::js_string(other.unwrap_or(&Value::Null)),
                (
                    relation.get("status").cloned().unwrap_or(Value::Null),
                    relation.get("requester_id").cloned().unwrap_or(Value::Null),
                ),
            );
        }

        let results: Vec<Value> = encontrados
            .into_iter()
            .map(|profile| {
                let id = profile.get("id").cloned().unwrap_or(Value::Null);
                let relation = rel_by_user.get(&util::js_string(&id));
                let status = relation
                    .map(|(status, _)| status.clone())
                    .unwrap_or(Value::Null);
                let incoming = relation
                    .map(|(_, requester)| *requester == id)
                    .unwrap_or(false);
                json!({
                    "id": id,
                    "username": profile.get("username").cloned().unwrap_or(Value::Null),
                    "display_name": profile.get("display_name").cloned().unwrap_or(Value::Null),
                    "avatar_url": profile.get("avatar_url").cloned().unwrap_or(Value::Null),
                    "status": status,
                    "incoming": incoming,
                })
            })
            .collect();

        Ok(json!({ "ok": true, "results": results }))
    }

    /// Envia pedido de amizade. Valida destino vazio/self como o JS.
    pub async fn send(&self, to_user_id: &str) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        if to_user_id.is_empty() || to_user_id == me {
            return Err(SocialError::InvalidParams("destino_invalido".into()));
        }

        let pair = canonical_pair(&me, to_user_id);
        self.client
            .from("friendships")
            .insert(json!({
                "user_a": pair.user_a,
                "user_b": pair.user_b,
                "requester_id": me,
                "status": "pending",
            }))
            .await
            .map_err(map_api_error)?;
        self.invalidate_cache();
        Ok(json!({ "ok": true }))
    }

    /// Aceita um pedido recebido (só o addressee, só `pending`).
    pub async fn accept(&self, friend_id: &str) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        let condition = format!(
            "and(user_a.eq.{me},user_b.eq.{friend_id}),and(user_a.eq.{friend_id},user_b.eq.{me})"
        );
        self.client
            .from("friendships")
            .update(json!({ "status": "accepted" }))
            .or(condition)
            .eq("status", "pending")
            .eq("requester_id", friend_id)
            .await
            .map_err(map_api_error)?;
        self.invalidate_cache();
        Ok(json!({ "ok": true }))
    }

    /// Cancela um pedido pendente enviado por mim.
    pub async fn cancel(&self, friend_id: &str) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        let condition = format!(
            "and(user_a.eq.{me},user_b.eq.{friend_id}),and(user_a.eq.{friend_id},user_b.eq.{me})"
        );
        self.client
            .from("friendships")
            .delete()
            .or(condition)
            .eq("status", "pending")
            .eq("requester_id", me)
            .await
            .map_err(map_api_error)?;
        self.invalidate_cache();
        Ok(json!({ "ok": true }))
    }

    /// Lista amigos/pedidos. Com cache válido (`< 60 s`) e `forcar = false`,
    /// devolve o cache imediatamente (`deCache: true`) e atualiza em background;
    /// o listener recebe o `data` fresco ao final. `forcar = true` sempre vai ao
    /// servidor.
    ///
    /// Envelope: `{ ok, data, deCache }`.
    pub async fn list(&self, forcar: bool) -> Result<Value, SocialError> {
        if !forcar {
            if let Some(data) = read_friends_cache(&self.cache_path()) {
                self.spawn_refresh();
                return Ok(json!({ "ok": true, "data": data, "deCache": true }));
            }
        }
        let data = self.fetch_fresh().await?;
        Ok(json!({ "ok": true, "data": data, "deCache": false }))
    }

    /// Força a busca no servidor e grava o cache (usado pelo refresh em
    /// background e por `list(forcar = true)`).
    pub async fn refresh_list(&self) -> Result<Value, SocialError> {
        self.fetch_fresh().await
    }

    /// Conquistas do amigo com o cache de 60 s de `friendAchievements`.
    ///
    /// Envelope: `{ ok: true, achievements: [...], deCache? }`.
    pub async fn friend_achievements(&self, friend_id: &str) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        if friend_id.is_empty() || friend_id == me {
            return Err(SocialError::InvalidParams("destino_invalido".into()));
        }

        let cache_path = self.achievements_cache_path();
        if let Some(cache) = read_friend_achievements(&cache_path, friend_id, FRIENDS_CACHE_TTL_MS)
        {
            return Ok(json!({
                "ok": true,
                "achievements": cache,
                "deCache": true,
            }));
        }

        let data = self
            .client
            .rpc("friend_achievements", json!({ "p_friend": friend_id }))
            .await
            .map_err(map_api_error)?;
        let achievements = if util::js_truthy(&data) {
            data
        } else {
            Value::Array(Vec::new())
        };
        let _ = write_friend_achievements(&cache_path, friend_id, &achievements);
        Ok(json!({ "ok": true, "achievements": achievements }))
    }

    /// Caminho usado pelo IPC (`friends:achievements`): tenta o cache de 5 min
    /// antes de repetir o round-trip ao servidor.
    pub async fn friend_achievements_cached(&self, friend_id: &str) -> Result<Value, SocialError> {
        let cache_path = self.achievements_cache_path();
        if let Some(cache) =
            read_friend_achievements(&cache_path, friend_id, FRIEND_ACHIEVEMENTS_TTL_MS)
        {
            return Ok(json!({
                "ok": true,
                "achievements": cache,
                "deCache": true,
            }));
        }
        self.friend_achievements(friend_id).await
    }

    /// Perfil do amigo pela RPC `friend_profile`. Como no JS não há checagem de
    /// sessão/self aqui; o backend decide o que pode ser exposto.
    pub async fn friend_profile(&self, friend_id: &str) -> Result<Value, SocialError> {
        let data = self
            .client
            .rpc("friend_profile", json!({ "p_friend": friend_id }))
            .await
            .map_err(map_api_error)?;
        if util::js_truthy(&data) {
            Ok(data)
        } else {
            Ok(json!({ "ok": false, "error": "perfil_indisponivel" }))
        }
    }

    /// Remove um amigo aceito; invalida o cache da lista e apaga as conquistas
    /// cacheadas daquele amigo (preservando os demais registros).
    pub async fn remove_friend(&self, friend_id: &str) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        if friend_id.is_empty() || friend_id == me {
            return Err(SocialError::InvalidParams("destino_invalido".into()));
        }

        let condition = format!(
            "and(user_a.eq.{me},user_b.eq.{friend_id}),and(user_a.eq.{friend_id},user_b.eq.{me})"
        );
        self.client
            .from("friendships")
            .delete()
            .or(condition)
            .eq("status", "accepted")
            .await
            .map_err(map_api_error)?;
        self.invalidate_cache();
        let _ = remove_friend_achievements(&self.achievements_cache_path(), friend_id);
        Ok(json!({ "ok": true }))
    }

    // ── interno ────────────────────────────────────────────────────────────

    async fn require_user_id(&self) -> Result<String, SocialError> {
        let Some(session) = self.client.auth().get_session().await else {
            return Err(SocialError::NotLoggedIn);
        };
        session
            .user_id()
            .map(str::to_string)
            .ok_or(SocialError::NotLoggedIn)
    }

    fn invalidate_cache(&self) {
        let _ = std::fs::remove_file(self.cache_path());
    }

    fn spawn_refresh(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            // Sem runtime não há como atualizar em background; o cache
            // continua sendo uma resposta válida para a primeira pintura.
            return;
        };
        let service = self.clone();
        handle.spawn(async move {
            if let Ok(data) = service.fetch_fresh().await {
                if let Some(listener) = &service.on_updated {
                    listener(&data);
                }
            }
        });
    }

    /// `buscarFresco`: uma query com os dois perfis embutidos.
    async fn fetch_fresh(&self) -> Result<Value, SocialError> {
        let me = self.require_user_id().await?;
        let rows = self
            .client
            .from("friendships")
            .select(FRIENDS_SELECT)
            .or(format!("user_a.eq.{me},user_b.eq.{me}"))
            .await
            .map_err(map_api_error)?;
        let data = build_friends_data(&me, &rows);
        // Cache é otimização: falha de disco nunca derruba a listagem.
        let _ = write_friends_cache(&self.cache_path(), &data);
        Ok(data)
    }
}

fn embedded(value: Option<&Value>) -> Option<&Value> {
    match value {
        Some(Value::Array(items)) => items.first(),
        Some(value) => Some(value),
        None => None,
    }
}

fn build_friends_data(me: &str, rows: &Value) -> Value {
    let mut friends = Vec::new();
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let mut profiles: HashMap<String, Option<Value>> = HashMap::new();

    for row in rows.as_array().into_iter().flatten() {
        let is_user_a_me = matches!(row.get("user_a"), Some(Value::String(value)) if value == me);
        let other = if is_user_a_me {
            row.get("user_b")
        } else {
            row.get("user_a")
        };
        let other_key = util::js_string(other.unwrap_or(&Value::Null));
        let pa = embedded(row.get("pa"));
        let pb = embedded(row.get("pb"));
        let profile = if is_user_a_me { pb } else { pa };
        let profile = profile
            .filter(|profile| util::js_truthy(profile))
            .map(|profile| {
                json!({
                    "username": profile
                        .get("username")
                        .filter(|value| util::js_truthy(value))
                        .cloned()
                        .unwrap_or_else(|| Value::String("?".into())),
                    "avatar_url": profile.get("avatar_url").cloned().unwrap_or(Value::Null),
                    "display_name": profile.get("display_name").cloned().unwrap_or(Value::Null),
                })
            });
        profiles.insert(other_key, profile);

        let info = (
            other.cloned().unwrap_or(Value::Null),
            row.get("created_at").cloned().unwrap_or(Value::Null),
        );
        if row.get("status").and_then(Value::as_str) == Some("accepted") {
            friends.push(info);
        } else if matches!(row.get("requester_id"), Some(Value::String(value)) if value == me) {
            outgoing.push(info);
        } else {
            incoming.push(info);
        }
    }

    let make = |info: &(Value, Value)| {
        let profile = profiles
            .get(&util::js_string(&info.0))
            .and_then(|profile| profile.as_ref());
        json!({
            "id": info.0,
            "username": profile
                .and_then(|profile| profile.get("username"))
                .cloned()
                .unwrap_or_else(|| Value::String("?".into())),
            "display_name": profile
                .and_then(|profile| profile.get("display_name"))
                .cloned()
                .unwrap_or(Value::Null),
            "avatar_url": profile
                .and_then(|profile| profile.get("avatar_url"))
                .cloned()
                .unwrap_or(Value::Null),
            "since": info.1,
        })
    };

    json!({
        "friends": friends.iter().map(make).collect::<Vec<_>>(),
        "incoming": incoming.iter().map(make).collect::<Vec<_>>(),
        "outgoing": outgoing.iter().map(make).collect::<Vec<_>>(),
    })
}

fn read_friends_cache(path: &Path) -> Option<Value> {
    let parsed = util::read_json(path)?;
    let timestamp = parsed.get("ts")?.as_f64()?;
    let data = parsed.get("data")?;
    if !util::js_truthy(data) {
        return None;
    }
    let age = util::now_millis() as f64 - timestamp;
    (age < FRIENDS_CACHE_TTL_MS as f64).then(|| data.clone())
}

fn write_friends_cache(path: &Path, data: &Value) -> io::Result<()> {
    // Preserva campos desconhecidos que uma versão futura tenha gravado.
    let mut cache = util::read_json_object(path);
    cache.insert("ts".into(), Value::from(util::now_millis()));
    cache.insert("data".into(), data.clone());
    util::write_json_atomic(path, &Value::Object(cache))
}

fn read_friend_achievements(path: &Path, friend_id: &str, ttl: i64) -> Option<Value> {
    let parsed = util::read_json(path)?;
    let entry = parsed.get(friend_id)?;
    let timestamp = entry.get("ts")?.as_f64()?;
    let data = entry.get("data")?;
    if !util::js_truthy(data) {
        return None;
    }
    let age = util::now_millis() as f64 - timestamp;
    (age < ttl as f64).then(|| data.clone())
}

fn write_friend_achievements(path: &Path, friend_id: &str, data: &Value) -> io::Result<()> {
    let mut cache = util::read_json_object(path);
    cache.insert(
        friend_id.to_string(),
        json!({ "ts": util::now_millis(), "data": data }),
    );
    util::write_json_atomic(path, &Value::Object(cache))
}

fn remove_friend_achievements(path: &Path, friend_id: &str) -> io::Result<()> {
    let mut cache = util::read_json_object(path);
    if cache.remove(friend_id).is_some() {
        util::write_json_atomic(path, &Value::Object(cache))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_pair_sorts_like_javascript() {
        assert_eq!(
            canonical_pair("b", "a"),
            CanonicalPair {
                user_a: "a".into(),
                user_b: "b".into()
            }
        );
        assert_eq!(canonical_pair("a", "b"), canonical_pair("b", "a"));
        assert_eq!(
            canonical_pair("x", "x"),
            CanonicalPair {
                user_a: "x".into(),
                user_b: "x".into()
            }
        );
        let a = "11111111-1111-1111-1111-111111111111";
        let b = "22222222-2222-2222-2222-222222222222";
        let pair = canonical_pair(b, a);
        assert_eq!(pair.user_a, a);
        assert_eq!(pair.user_b, b);
    }

    #[test]
    fn friends_data_splits_statuses_and_uses_embedded_profile_side() {
        let rows = json!([
            {
                "user_a": "me",
                "user_b": "friend-1",
                "status": "accepted",
                "requester_id": "friend-1",
                "created_at": "2024-01-01T00:00:00Z",
                "pa": { "id": "me", "username": "eu", "avatar_url": null, "display_name": null },
                "pb": [{ "id": "friend-1", "username": "amigo", "avatar_url": "a.png", "display_name": "Amigo" }]
            },
            {
                "user_a": "incoming-1",
                "user_b": "me",
                "status": "pending",
                "requester_id": "incoming-1",
                "created_at": "2024-01-02T00:00:00Z",
                "pa": { "id": "incoming-1", "username": "pedinte" },
                "pb": { "id": "me", "username": "eu" }
            },
            {
                "user_a": "me",
                "user_b": "outgoing-1",
                "status": "pending",
                "requester_id": "me",
                "created_at": null,
                "pa": { "id": "me", "username": "eu" },
                "pb": null
            }
        ]);
        let data = build_friends_data("me", &rows);
        assert_eq!(data["friends"][0]["id"], "friend-1");
        assert_eq!(data["friends"][0]["username"], "amigo");
        assert_eq!(data["friends"][0]["avatar_url"], "a.png");
        assert_eq!(data["friends"][0]["since"], "2024-01-01T00:00:00Z");
        assert_eq!(data["incoming"][0]["username"], "pedinte");
        assert_eq!(data["incoming"][0]["display_name"], Value::Null);
        // pb = null vira username "?" e since null.
        assert_eq!(data["outgoing"][0]["username"], "?");
        assert_eq!(data["outgoing"][0]["since"], Value::Null);
    }
}
