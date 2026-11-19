//! Testes de contrato de amigos/comunidade contra um servidor HTTP local
//! (wiremock). Nenhuma chamada ao backend real; arquivos ficam em diretórios
//! temporários.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arcadia_api::{ApiClient, ApiConfig, Session, DEFAULT_ANON_KEY};
use arcadia_social::{
    canonical_pair, CommunityService, FriendsService, PageOptions, SocialError,
    COMMUNITY_CACHE_FILE, FRIENDS_CACHE_FILE, FRIEND_ACHIEVEMENTS_CACHE_FILE,
};
use arcadia_store::accounts::AccountScope;
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// ── fixtures ───────────────────────────────────────────────────────────────

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "arcadia-social-it-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scope_for(dir: &Path) -> AccountScope {
    AccountScope::with_active(dir, Some("u1"))
}

fn account_file(dir: &Path, name: &str) -> PathBuf {
    dir.join("contas").join("u1").join(name)
}

fn client_for(server: &MockServer) -> ApiClient {
    ApiClient::new(ApiConfig::resolve(Some(&server.uri()), None, None, None)).unwrap()
}

fn session_with_token(token: &str, refresh: Option<&str>) -> Session {
    serde_json::from_value(json!({
        "access_token": token,
        "refresh_token": refresh,
        "expires_at": 4_000_000_000u64,
        "token_type": "bearer",
        "user": {
            "id": "user-1",
            "email": "user@arcadia.test",
            "user_metadata": { "username": "u1" }
        }
    }))
    .unwrap()
}

async fn install_session(client: &ApiClient) {
    client
        .auth()
        .set_session_optimistic(session_with_token("test-token", Some("refresh-1")), false)
        .await;
}

fn auth_header() -> wiremock::matchers::HeaderExactMatcher {
    header("authorization", "Bearer test-token")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_string(value).unwrap()).unwrap();
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn query_pairs(request: &Request) -> HashMap<String, String> {
    request
        .url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

fn find_request<'a>(requests: &'a [Request], path: &str, method_name: &str) -> &'a Request {
    requests
        .iter()
        .find(|request| request.url.path() == path && request.method.as_str() == method_name)
        .unwrap_or_else(|| panic!("request {method_name} {path} não encontrada: {requests:?}"))
}

fn dead_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

fn json_response(status: u16, value: Value) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(value)
}

// ── amigos ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn friends_search_uses_exact_query_and_marks_relations() {
    let server = MockServer::start().await;
    let dir = temp_dir("search");
    let client = client_for(&server);
    install_session(&client).await;
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/rest/v1/profiles"))
        .and(query_param("select", "id, username, avatar_url, display_name"))
        .and(query_param("username", "ilike.ana%"))
        .and(query_param("limit", "20"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .and(auth_header())
        .respond_with(json_response(
            200,
            json!([
                { "id": "user-1", "username": "ana-eu", "avatar_url": null, "display_name": null },
                { "id": "friend-1", "username": "ana", "avatar_url": "a.png", "display_name": "Ana" },
                { "id": "friend-2", "username": "ana2", "avatar_url": null, "display_name": null }
            ]),
        ))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("select", "user_a, user_b, status, requester_id"))
        .and(query_param("or", "(user_a.eq.user-1,user_b.eq.user-1)"))
        .and(auth_header())
        .respond_with(json_response(
            200,
            json!([
                { "user_a": "user-1", "user_b": "friend-1", "status": "accepted", "requester_id": "friend-1" },
                { "user_a": "friend-2", "user_b": "user-1", "status": "pending", "requester_id": "friend-2" }
            ]),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let result = service.search("  AnA ").await.unwrap();
    assert_eq!(
        result,
        json!({
            "ok": true,
            "results": [
                {
                    "id": "friend-1",
                    "username": "ana",
                    "display_name": "Ana",
                    "avatar_url": "a.png",
                    "status": "accepted",
                    "incoming": true
                },
                {
                    "id": "friend-2",
                    "username": "ana2",
                    "display_name": null,
                    "avatar_url": null,
                    "status": "pending",
                    "incoming": true
                }
            ]
        })
    );

    let requests = server.received_requests().await.unwrap();
    let profiles = find_request(&requests, "/rest/v1/profiles", "GET");
    assert_eq!(
        query_pairs(profiles).get("username").map(String::as_str),
        Some("ilike.ana%")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_search_skips_relation_query_when_only_self_matches() {
    let server = MockServer::start().await;
    let dir = temp_dir("search-self");
    let client = client_for(&server);
    install_session(&client).await;
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/rest/v1/profiles"))
        .respond_with(json_response(
            200,
            json!([{ "id": "user-1", "username": "eu" }]),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/friendships"))
        .respond_with(json_response(200, json!([])))
        .expect(0)
        .mount(&server)
        .await;

    assert_eq!(
        service.search("eu").await.unwrap(),
        json!({ "ok": true, "results": [] })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_send_canonicalizes_pair_invalidates_cache_and_validates_destination() {
    let server = MockServer::start().await;
    let dir = temp_dir("send");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, FRIENDS_CACHE_FILE);
    write_json(
        &cache,
        &json!({ "ts": now_millis(), "data": { "friends": [] } }),
    );
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("POST"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("select", "*"))
        .and(auth_header())
        .and(body_json(json!({
            "user_a": "friend-0",
            "user_b": "user-1",
            "requester_id": "user-1",
            "status": "pending"
        })))
        .respond_with(json_response(201, json!([])))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        service.send("friend-0").await.unwrap(),
        json!({ "ok": true })
    );
    assert!(!cache.exists(), "cache deve ser invalidado após enviar");

    let invalid = service.send("user-1").await.unwrap_err();
    assert_eq!(invalid.code(), "parametros_invalidos");
    assert_eq!(
        invalid.to_friends_envelope(),
        json!({ "ok": false, "error": "destino_invalido" })
    );
    assert_eq!(
        service.send("").await.unwrap_err().code(),
        "parametros_invalidos"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_accept_and_cancel_send_exact_filters() {
    let server = MockServer::start().await;
    let dir = temp_dir("accept-cancel");
    let client = client_for(&server);
    install_session(&client).await;
    let service = FriendsService::new(client, scope_for(&dir));

    let condition =
        "(and(user_a.eq.user-1,user_b.eq.friend-1),and(user_a.eq.friend-1,user_b.eq.user-1))";
    Mock::given(method("PATCH"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("or", condition))
        .and(query_param("status", "eq.pending"))
        .and(query_param("requester_id", "eq.friend-1"))
        .and(auth_header())
        .and(body_json(json!({ "status": "accepted" })))
        .respond_with(json_response(200, json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("or", condition))
        .and(query_param("status", "eq.pending"))
        .and(query_param("requester_id", "eq.user-1"))
        .and(auth_header())
        .respond_with(json_response(200, json!([])))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        service.accept("friend-1").await.unwrap(),
        json!({ "ok": true })
    );
    assert_eq!(
        service.cancel("friend-1").await.unwrap(),
        json!({ "ok": true })
    );

    let requests = server.received_requests().await.unwrap();
    let delete = find_request(&requests, "/rest/v1/friendships", "DELETE");
    assert!(delete.body.is_empty(), "DELETE não envia corpo");
    let _ = std::fs::remove_dir_all(&dir);
}

const FRIENDS_SELECT: &str = "user_a, user_b, status, requester_id, created_at,\n       pa:profiles!friendships_user_a_fkey(id, username, avatar_url, display_name),\n       pb:profiles!friendships_user_b_fkey(id, username, avatar_url, display_name)";

#[tokio::test]
async fn friends_list_fresh_uses_embedded_query_and_preserves_unknown_cache_fields() {
    let server = MockServer::start().await;
    let dir = temp_dir("list-fresh");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, FRIENDS_CACHE_FILE);
    // Cache expirado com campo desconhecido: a gravação fresca o preserva.
    write_json(
        &cache,
        &json!({ "ts": 0, "data": null, "futuro": { "keep": true } }),
    );
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("select", FRIENDS_SELECT))
        .and(query_param("or", "(user_a.eq.user-1,user_b.eq.user-1)"))
        .and(auth_header())
        .respond_with(json_response(
            200,
            json!([
                {
                    "user_a": "user-1",
                    "user_b": "friend-1",
                    "status": "accepted",
                    "requester_id": "friend-1",
                    "created_at": "2024-01-01T00:00:00Z",
                    "pa": { "id": "user-1", "username": "eu", "avatar_url": null, "display_name": null },
                    "pb": [{ "id": "friend-1", "username": "amigo", "avatar_url": "a.png", "display_name": "Amigo" }]
                },
                {
                    "user_a": "incoming-1",
                    "user_b": "user-1",
                    "status": "pending",
                    "requester_id": "incoming-1",
                    "created_at": "2024-01-02T00:00:00Z",
                    "pa": { "id": "incoming-1", "username": "pedinte" },
                    "pb": { "id": "user-1", "username": "eu" }
                },
                {
                    "user_a": "user-1",
                    "user_b": "outgoing-1",
                    "status": "pending",
                    "requester_id": "user-1",
                    "created_at": null,
                    "pa": { "id": "user-1", "username": "eu" },
                    "pb": null
                }
            ]),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let result = service.list(false).await.unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["deCache"], false);
    assert_eq!(result["data"]["friends"][0]["id"], "friend-1");
    assert_eq!(result["data"]["friends"][0]["username"], "amigo");
    assert_eq!(
        result["data"]["friends"][0]["since"],
        "2024-01-01T00:00:00Z"
    );
    assert_eq!(result["data"]["incoming"][0]["username"], "pedinte");
    assert_eq!(result["data"]["outgoing"][0]["username"], "?");
    assert_eq!(result["data"]["outgoing"][0]["since"], Value::Null);

    let cached = read_json(&cache);
    assert_eq!(cached["futuro"], json!({ "keep": true }));
    assert!(cached["ts"].as_i64().unwrap() > 0);
    assert_eq!(cached["data"], result["data"]);

    let requests = server.received_requests().await.unwrap();
    let list = find_request(&requests, "/rest/v1/friendships", "GET");
    assert_eq!(
        query_pairs(list).get("select").map(String::as_str),
        Some(FRIENDS_SELECT)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_list_cache_hit_returns_immediately_and_emits_background_refresh() {
    let server = MockServer::start().await;
    let dir = temp_dir("list-cache");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, FRIENDS_CACHE_FILE);
    let cached_data = json!({
        "friends": [{ "id": "old", "username": "antigo", "display_name": null, "avatar_url": null, "since": null }],
        "incoming": [],
        "outgoing": []
    });
    write_json(
        &cache,
        &json!({ "ts": now_millis(), "data": cached_data, "futuro": "keep" }),
    );

    Mock::given(method("GET"))
        .and(path("/rest/v1/friendships"))
        .respond_with(json_response(
            200,
            json!([{
                "user_a": "user-1",
                "user_b": "new",
                "status": "accepted",
                "requester_id": "new",
                "created_at": "2024-02-02T00:00:00Z",
                "pa": { "id": "user-1", "username": "eu" },
                "pb": { "id": "new", "username": "novo", "avatar_url": null, "display_name": null }
            }]),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let mut service = FriendsService::new(client, scope_for(&dir));
    service.set_updated_listener(move |data| {
        let _ = sender.try_send(data.clone());
    });

    let result = service.list(false).await.unwrap();
    assert_eq!(result["deCache"], true);
    assert_eq!(result["data"], cached_data);

    let fresh = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .expect("background refresh não emitiu friends:changed")
        .expect("listener fechado");
    assert_eq!(fresh["friends"][0]["id"], "new");

    let cached = read_json(&cache);
    assert_eq!(cached["futuro"], "keep");
    assert_eq!(cached["data"]["friends"][0]["id"], "new");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_list_forcar_ignores_cache() {
    let server = MockServer::start().await;
    let dir = temp_dir("list-forcar");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, FRIENDS_CACHE_FILE);
    write_json(
        &cache,
        &json!({ "ts": now_millis(), "data": { "friends": [], "incoming": [], "outgoing": [] } }),
    );

    Mock::given(method("GET"))
        .and(path("/rest/v1/friendships"))
        .respond_with(json_response(
            200,
            json!([{
                "user_a": "user-1",
                "user_b": "fresh",
                "status": "accepted",
                "requester_id": "fresh",
                "created_at": null,
                "pa": { "id": "user-1", "username": "eu" },
                "pb": { "id": "fresh", "username": "fresco" }
            }]),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let service = FriendsService::new(client, scope_for(&dir));
    let result = service.list(true).await.unwrap();
    assert_eq!(result["deCache"], false);
    assert_eq!(result["data"]["friends"][0]["id"], "fresh");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_achievements_uses_60s_cache_and_preserves_other_entries() {
    let server = MockServer::start().await;
    let dir = temp_dir("achievements");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, FRIEND_ACHIEVEMENTS_CACHE_FILE);
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/friend_achievements"))
        .and(auth_header())
        .and(body_json(json!({ "p_friend": "friend-1" })))
        .respond_with(json_response(
            200,
            json!([{ "appid": "10", "apiname": "A" }]),
        ))
        .expect(2)
        .mount(&server)
        .await;

    let first = service.friend_achievements("friend-1").await.unwrap();
    assert_eq!(first["deCache"], Value::Null);
    assert_eq!(first["achievements"][0]["apiname"], "A");

    let cached = service.friend_achievements("friend-1").await.unwrap();
    assert_eq!(cached["deCache"], true);
    assert_eq!(cached["achievements"][0]["apiname"], "A");

    // Expirado (61 s) volta ao servidor; os demais registros sobrevivem.
    let mut all = read_json(&cache);
    all["friend-1"]["ts"] = json!(now_millis() - 61_000);
    all["friend-2"] = json!({ "ts": now_millis(), "data": [{ "apiname": "B" }] });
    all["futuro"] = json!({ "keep": true });
    write_json(&cache, &all);

    let refreshed = service.friend_achievements("friend-1").await.unwrap();
    assert_eq!(refreshed["deCache"], Value::Null);
    let all = read_json(&cache);
    assert_eq!(all["friend-2"]["data"][0]["apiname"], "B");
    assert_eq!(all["futuro"], json!({ "keep": true }));

    let invalid = service.friend_achievements("user-1").await.unwrap_err();
    assert_eq!(invalid.code(), "parametros_invalidos");
    assert_eq!(
        service.friend_achievements("").await.unwrap_err().code(),
        "parametros_invalidos"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_achievements_cached_uses_five_minute_ttl() {
    let server = MockServer::start().await;
    let dir = temp_dir("achievements-cached");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, FRIEND_ACHIEVEMENTS_CACHE_FILE);
    write_json(
        &cache,
        &json!({
            "friend-1": { "ts": now_millis() - 120_000, "data": [{ "apiname": "CACHE" }] },
            "friend-2": { "ts": now_millis() - 120_000, "data": [{ "apiname": "OUTRO" }] }
        }),
    );
    let service = FriendsService::new(client, scope_for(&dir));

    let cached = service
        .friend_achievements_cached("friend-1")
        .await
        .unwrap();
    assert_eq!(cached["deCache"], true);
    assert_eq!(cached["achievements"][0]["apiname"], "CACHE");

    let mut all = read_json(&cache);
    all["friend-1"]["ts"] = json!(now_millis() - 301_000);
    write_json(&cache, &all);

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/friend_achievements"))
        .and(body_json(json!({ "p_friend": "friend-1" })))
        .respond_with(json_response(200, json!([{ "apiname": "FRESCO" }])))
        .expect(1)
        .mount(&server)
        .await;

    let fresh = service
        .friend_achievements_cached("friend-1")
        .await
        .unwrap();
    assert_eq!(fresh["deCache"], Value::Null);
    assert_eq!(fresh["achievements"][0]["apiname"], "FRESCO");
    let all = read_json(&cache);
    assert_eq!(all["friend-2"]["data"][0]["apiname"], "OUTRO");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_profile_maps_rpc_and_null_fallback() {
    let server = MockServer::start().await;
    let dir = temp_dir("profile");
    let client = client_for(&server);
    // Sem sessão: friend_profile não exige login, como no JS.
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/friend_profile"))
        .and(body_json(json!({ "p_friend": "friend-1" })))
        .respond_with(json_response(
            200,
            json!({ "ok": true, "profile": { "username": "amigo" } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/friend_profile"))
        .and(body_json(json!({ "p_friend": "friend-2" })))
        .respond_with(json_response(200, Value::Null))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        service.friend_profile("friend-1").await.unwrap(),
        json!({ "ok": true, "profile": { "username": "amigo" } })
    );
    assert_eq!(
        service.friend_profile("friend-2").await.unwrap(),
        json!({ "ok": false, "error": "perfil_indisponivel" })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_remove_filters_accepted_and_prunes_achievement_cache() {
    let server = MockServer::start().await;
    let dir = temp_dir("remove");
    let client = client_for(&server);
    install_session(&client).await;
    let friends_cache = account_file(&dir, FRIENDS_CACHE_FILE);
    let achievements_cache = account_file(&dir, FRIEND_ACHIEVEMENTS_CACHE_FILE);
    write_json(
        &friends_cache,
        &json!({ "ts": now_millis(), "data": { "friends": [] } }),
    );
    write_json(
        &achievements_cache,
        &json!({
            "friend-1": { "ts": now_millis(), "data": [] },
            "friend-2": { "ts": now_millis(), "data": [{ "apiname": "B" }] },
            "futuro": { "keep": true }
        }),
    );
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("DELETE"))
        .and(path("/rest/v1/friendships"))
        .and(query_param(
            "or",
            "(and(user_a.eq.user-1,user_b.eq.friend-1),and(user_a.eq.friend-1,user_b.eq.user-1))",
        ))
        .and(query_param("status", "eq.accepted"))
        .and(auth_header())
        .respond_with(json_response(200, json!([])))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        service.remove_friend("friend-1").await.unwrap(),
        json!({ "ok": true })
    );
    assert!(!friends_cache.exists());
    let achievements = read_json(&achievements_cache);
    assert!(achievements.get("friend-1").is_none());
    assert_eq!(achievements["friend-2"]["data"][0]["apiname"], "B");
    assert_eq!(achievements["futuro"], json!({ "keep": true }));

    assert_eq!(
        service.remove_friend("user-1").await.unwrap_err().code(),
        "parametros_invalidos"
    );
    assert_eq!(
        service.remove_friend("").await.unwrap_err().code(),
        "parametros_invalidos"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_operations_require_a_session() {
    let server = MockServer::start().await;
    let dir = temp_dir("not-logged");
    let client = client_for(&server);
    let service = FriendsService::new(client, scope_for(&dir));

    assert_eq!(service.search("x").await.unwrap_err().code(), "nao_logado");
    assert_eq!(
        service.send("friend-1").await.unwrap_err().code(),
        "nao_logado"
    );
    assert_eq!(
        service.accept("friend-1").await.unwrap_err().code(),
        "nao_logado"
    );
    assert_eq!(
        service.cancel("friend-1").await.unwrap_err().code(),
        "nao_logado"
    );
    assert_eq!(service.list(false).await.unwrap_err().code(), "nao_logado");
    assert_eq!(
        service.remove_friend("friend-1").await.unwrap_err().code(),
        "nao_logado"
    );
    // Nenhuma requisição saiu.
    assert!(server.received_requests().await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_401_maps_to_token_invalido_with_backend_message() {
    let server = MockServer::start().await;
    let dir = temp_dir("friends-401");
    let client = client_for(&server);
    client
        .auth()
        .set_session_optimistic(session_with_token("expired", None), false)
        .await;
    let service = FriendsService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/rest/v1/profiles"))
        .and(header("authorization", "Bearer expired"))
        .respond_with(json_response(401, json!({ "error": "token_expirado" })))
        .expect(1)
        .mount(&server)
        .await;

    let error = service.search("x").await.unwrap_err();
    assert_eq!(error.code(), "token_invalido");
    assert_eq!(
        error.to_friends_envelope(),
        json!({ "ok": false, "error": "token_expirado" })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn friends_transport_failure_is_rede_indisponivel() {
    let dir = temp_dir("friends-network");
    let client = ApiClient::new(ApiConfig::resolve(Some(&dead_url()), None, None, None)).unwrap();
    client
        .auth()
        .set_session_optimistic(session_with_token("test-token", None), false)
        .await;
    let service = FriendsService::new(client, scope_for(&dir));
    let error = service.search("x").await.unwrap_err();
    assert_eq!(error.code(), "rede_indisponivel");
    assert_eq!(
        error.to_friends_envelope(),
        json!({ "ok": false, "error": "rede_indisponivel" })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── comunidade ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn community_list_reviews_exact_request_envelope_and_cache() {
    let server = MockServer::start().await;
    let dir = temp_dir("reviews");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, COMMUNITY_CACHE_FILE);
    write_json(&cache, &json!({ "version": 1, "futuro": { "keep": true } }));
    let service = CommunityService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/community/v1/reviews"))
        .and(query_param("appid", "steam:10"))
        .and(query_param("limit", "10"))
        .and(query_param("offset", "20"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .and(auth_header())
        .respond_with(json_response(
            200,
            json!({
                "reviews": [{ "id": 3, "appid": "steam:10", "text": "bom", "rating": 5 }],
                "pagination": { "limit": 10, "offset": 20, "has_more": false }
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let options = PageOptions::from_value(&json!({ "limit": 10, "offset": 20 }));
    let result = service.list_reviews("steam:10", &options).await.unwrap();
    assert_eq!(
        result,
        json!({
            "ok": true,
            "reviews": [{ "id": 3, "appid": "steam:10", "text": "bom", "rating": 5 }],
            "items": [{ "id": 3, "appid": "steam:10", "text": "bom", "rating": 5 }],
            "pagination": { "limit": 10, "offset": 20, "has_more": false }
        })
    );

    let cached = read_json(&cache);
    assert_eq!(cached["version"], 1);
    assert_eq!(cached["futuro"], json!({ "keep": true }));
    assert_eq!(cached["reviews"]["reviews:steam:10"]["reviews"][0]["id"], 3);
    assert_eq!(cached["reviews"]["reviews:steam:10"]["items"][0]["id"], 3);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].url.query(),
        Some("appid=steam%3A10&limit=10&offset=20")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_list_reviews_serves_cache_offline_and_uses_requested_page_only_for_missing_pagination(
) {
    let server = MockServer::start().await;
    let dir = temp_dir("reviews-offline");
    let client = client_for(&server);
    install_session(&client).await;
    let service = CommunityService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/community/v1/reviews"))
        .respond_with(json_response(
            200,
            json!({
                "reviews": [{ "id": 1, "appid": "x", "text": "cache" }],
                "pagination": { "limit": 1, "offset": 0, "has_more": false }
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let first = service
        .list_reviews("x", &PageOptions::default())
        .await
        .unwrap();
    assert_eq!(first["reviews"][0]["text"], "cache");

    // Servidor cai: o cache responde com offline:true.
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/community/v1/reviews"))
        .respond_with(json_response(503, json!({ "error": "indisponivel" })))
        .mount(&server)
        .await;
    let offline = service
        .list_reviews(
            "x",
            &PageOptions::from_value(&json!({ "limit": 5, "offset": 10 })),
        )
        .await
        .unwrap();
    assert_eq!(offline["ok"], true);
    assert_eq!(offline["offline"], true);
    assert_eq!(offline["reviews"][0]["text"], "cache");
    // Paginação existente no cache é preservada.
    assert_eq!(offline["pagination"]["limit"], 1);

    // Sem cache para outro appid: erro tipado preservando o corpo.
    let error = service
        .list_reviews("sem-cache", &PageOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "indisponivel");
    assert_eq!(
        error.to_community_envelope(),
        json!({
            "ok": false,
            "error": "indisponivel",
            "code": "indisponivel",
            "message": "indisponivel",
            "status": 503
        })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_review_mutations_use_exact_paths_and_update_cache() {
    let server = MockServer::start().await;
    let dir = temp_dir("review-mutations");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, COMMUNITY_CACHE_FILE);
    let service = CommunityService::new(client, scope_for(&dir));

    Mock::given(method("POST"))
        .and(path("/community/v1/reviews"))
        .and(auth_header())
        .and(body_json(
            json!({ "appid": "steam:42", "text": "ok", "rating": 5 }),
        ))
        .respond_with(json_response(
            201,
            json!({ "review": { "id": 9, "appid": "steam:42", "text": "ok", "rating": 5 } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/community/v1/reviews/id%2Fwith%20space"))
        .and(auth_header())
        .and(body_json(json!({ "text": "editado" })))
        .respond_with(json_response(
            200,
            json!({ "data": { "id": 9, "appid": "steam:42", "text": "editado", "rating": 4 } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/community/v1/reviews/9"))
        .and(auth_header())
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/community/v1/reviews/9/report"))
        .and(auth_header())
        .and(body_json(json!({ "reason": "spam" })))
        .respond_with(json_response(200, json!({ "report": { "id": 1 } })))
        .expect(1)
        .mount(&server)
        .await;

    let created = service
        .create_review(&json!({ "appid": "steam:42", "text": "ok", "rating": 5 }))
        .await
        .unwrap();
    assert_eq!(
        created,
        json!({
            "ok": true,
            "review": { "id": 9, "appid": "steam:42", "text": "ok", "rating": 5 },
            "data": { "id": 9, "appid": "steam:42", "text": "ok", "rating": 5 }
        })
    );
    let cached = read_json(&cache);
    assert_eq!(cached["reviews"]["reviews:steam:42"]["reviews"][0]["id"], 9);

    let updated = service
        .update_review("id/with space", &json!({ "text": "editado" }))
        .await
        .unwrap();
    assert_eq!(updated["review"]["text"], "editado");
    assert_eq!(updated["data"]["text"], "editado");
    let cached = read_json(&cache);
    assert_eq!(
        cached["reviews"]["reviews:steam:42"]["reviews"][0]["text"],
        "editado"
    );

    assert_eq!(
        service.remove_review("9").await.unwrap(),
        json!({ "ok": true })
    );
    let cached = read_json(&cache);
    assert_eq!(cached["reviews"]["reviews:steam:42"]["reviews"], json!([]));

    let reported = service
        .report_review("9", &json!({ "reason": "spam" }))
        .await
        .unwrap();
    assert_eq!(reported, json!({ "ok": true, "report": { "id": 1 } }));

    let requests = server.received_requests().await.unwrap();
    let delete = find_request(&requests, "/community/v1/reviews/9", "DELETE");
    assert!(delete.body.is_empty());
    assert!(delete.headers.get("content-type").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_list_collections_uses_exact_filters_and_offline_cache() {
    let server = MockServer::start().await;
    let dir = temp_dir("collections");
    let client = client_for(&server);
    install_session(&client).await;
    let cache = account_file(&dir, COMMUNITY_CACHE_FILE);
    let service = CommunityService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/community/v1/collections"))
        .and(query_param("limit", "10"))
        .and(query_param("offset", "0"))
        .and(query_param("mine", "true"))
        .and(query_param("owner", "u1"))
        .and(query_param("visibility", "public"))
        .and(auth_header())
        .respond_with(json_response(
            200,
            json!({ "collections": [{ "id": "c1", "title": "Minha", "owner_id": "u1" }] }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let options = PageOptions::from_value(&json!({
        "limit": 10,
        "mine": true,
        "user_id": "u1",
        "visibility": "public"
    }));
    let result = service.list_collections(&options).await.unwrap();
    assert_eq!(
        result,
        json!({
            "ok": true,
            "collections": [{ "id": "c1", "title": "Minha", "owner_id": "u1" }],
            "lists": [{ "id": "c1", "title": "Minha", "owner_id": "u1" }],
            "items": [{ "id": "c1", "title": "Minha", "owner_id": "u1" }],
            "pagination": { "limit": 1, "offset": 0, "has_more": false }
        })
    );

    let cached = read_json(&cache);
    let key = r#"{"limit":10,"offset":0,"mine":true,"owner":"u1","visibility":"public"}"#;
    assert!(cached["collections"].get(key).is_some(), "cache: {cached}");

    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/community/v1/collections"))
        .respond_with(json_response(500, json!({ "error": "falhou" })))
        .mount(&server)
        .await;
    let offline = service.list_collections(&options).await.unwrap();
    assert_eq!(offline["ok"], true);
    assert_eq!(offline["offline"], true);
    assert_eq!(offline["collections"][0]["id"], "c1");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].url.query(),
        Some("limit=10&offset=0&mine=true&owner=u1&visibility=public")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_collection_crud_and_item_routes() {
    let server = MockServer::start().await;
    let dir = temp_dir("collection-crud");
    let client = client_for(&server);
    install_session(&client).await;
    let service = CommunityService::new(client, scope_for(&dir));

    Mock::given(method("POST"))
        .and(path("/community/v1/collections"))
        .and(auth_header())
        .and(body_json(
            json!({ "title": "Favoritos", "visibility": "public" }),
        ))
        .respond_with(json_response(
            201,
            json!({ "collection": { "id": "c/1", "title": "Favoritos" } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/community/v1/collections/c%2F1"))
        .and(auth_header())
        .respond_with(json_response(
            200,
            json!({ "data": { "id": "c/1", "title": "Favoritos", "items": [] } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/community/v1/collections/c%2F1"))
        .and(auth_header())
        .and(body_json(json!({ "title": "Novo" })))
        .respond_with(json_response(
            200,
            json!({ "collection": { "id": "c/1", "title": "Novo" } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/community/v1/collections/c%2F1/items"))
        .and(auth_header())
        .and(body_json(json!({ "appid": "steam:42" })))
        .respond_with(json_response(
            201,
            json!({ "ok": true, "items": [{ "appid": "steam:42", "position": 0 }] }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/community/v1/collections/c%2F1/items"))
        .and(auth_header())
        .and(body_json(
            json!({ "items": [{ "appid": "steam:1" }, { "appid": "steam:2" }] }),
        ))
        .respond_with(json_response(
            200,
            json!({ "collection": { "id": "c/1" }, "items": [{ "appid": "steam:1" }] }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/community/v1/collections/c%2F1/items/steam%3A42"))
        .and(auth_header())
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/community/v1/collections/c%2F1"))
        .and(auth_header())
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/community/v1/collections/c%2F1/report"))
        .and(auth_header())
        .and(body_json(json!({ "reason": "ofensiva" })))
        .respond_with(json_response(200, json!({ "report": { "id": 7 } })))
        .expect(1)
        .mount(&server)
        .await;

    let created = service
        .create_collection(&json!({ "title": "Favoritos", "visibility": "public" }))
        .await
        .unwrap();
    assert_eq!(
        created,
        json!({
            "ok": true,
            "collection": { "id": "c/1", "title": "Favoritos" },
            "data": { "id": "c/1", "title": "Favoritos" }
        })
    );

    let fetched = service.get_collection("c/1").await.unwrap();
    assert_eq!(fetched["ok"], true);
    assert_eq!(fetched["collection"]["id"], "c/1");
    assert_eq!(fetched["data"]["title"], "Favoritos");
    // getCollection grava o item no cache.
    let cache_path = account_file(&dir, COMMUNITY_CACHE_FILE);
    let cached = read_json(&cache_path);
    assert_eq!(cached["collectionItems"]["c/1"]["id"], "c/1");

    let updated = service
        .update_collection("c/1", &json!({ "title": "Novo" }))
        .await
        .unwrap();
    assert_eq!(updated["collection"]["title"], "Novo");

    let added = service
        .add_collection_item("c/1", "steam:42")
        .await
        .unwrap();
    assert_eq!(added["ok"], true);
    assert_eq!(added["items"][0]["appid"], "steam:42");
    assert_eq!(added["data"]["items"][0]["appid"], "steam:42");

    let replaced = service
        .replace_collection_items(
            "c/1",
            &[json!({ "appid": "steam:1" }), json!({ "appid": "steam:2" })],
        )
        .await
        .unwrap();
    assert_eq!(replaced["collection"]["id"], "c/1");
    assert_eq!(replaced["items"][0]["appid"], "steam:1");

    let removed_item = service
        .remove_collection_item("c/1", "steam:42")
        .await
        .unwrap();
    assert_eq!(removed_item, json!({ "ok": true, "data": null }));

    let reported = service
        .report_collection("c/1", &json!({ "reason": "ofensiva" }))
        .await
        .unwrap();
    assert_eq!(reported, json!({ "ok": true, "report": { "id": 7 } }));

    assert_eq!(
        service.remove_collection("c/1").await.unwrap(),
        json!({ "ok": true })
    );
    let requests = server.received_requests().await.unwrap();
    let remove_item = find_request(
        &requests,
        "/community/v1/collections/c%2F1/items/steam%3A42",
        "DELETE",
    );
    assert!(remove_item.body.is_empty());
    assert!(remove_item.headers.get("content-type").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_401_and_403_are_propagated_with_backend_fields() {
    let server = MockServer::start().await;
    let dir = temp_dir("community-auth");
    let client = client_for(&server);
    install_session(&client).await;
    let service = CommunityService::new(client, scope_for(&dir));

    Mock::given(method("GET"))
        .and(path("/community/v1/reviews"))
        .and(query_param("appid", "x"))
        .respond_with(json_response(
            401,
            json!({
                "error": "token_expirado",
                "code": "invalid_token",
                "message": "Sessão expirada"
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/community/v1/reviews/9"))
        .respond_with(json_response(
            403,
            json!({
                "error": "sem_permissao",
                "code": "42501",
                "request_id": "req-1"
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let error = service
        .list_reviews("x", &PageOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "token_invalido");
    assert_eq!(
        error.to_community_envelope(),
        json!({
            "ok": false,
            "error": "token_expirado",
            "code": "invalid_token",
            "message": "Sessão expirada",
            "status": 401
        })
    );

    let forbidden = service
        .update_review("9", &json!({ "text": "x" }))
        .await
        .unwrap_err();
    assert_eq!(forbidden.code(), "42501");
    assert_eq!(
        forbidden.to_community_envelope(),
        json!({
            "ok": false,
            "error": "sem_permissao",
            "code": "42501",
            "message": "sem_permissao",
            "request_id": "req-1",
            "status": 403
        })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_transport_failure_is_offline_envelope() {
    let dir = temp_dir("community-network");
    let client = ApiClient::new(ApiConfig::resolve(Some(&dead_url()), None, None, None)).unwrap();
    let service = CommunityService::new(client, scope_for(&dir));
    let error = service
        .list_reviews("x", &PageOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "rede_indisponivel");
    let envelope = error.to_community_envelope();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["offline"], true);
    assert_eq!(envelope["error"], "offline");
    assert_eq!(envelope["code"], "offline");
    assert_eq!(envelope["status"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn community_account_switch_discards_the_response_and_never_writes_new_account_cache() {
    let server = MockServer::start().await;
    let dir = temp_dir("community-switch");
    let client = client_for(&server);
    install_session(&client).await;
    let service = CommunityService::new(client, scope_for(&dir));
    let notify = Arc::new(tokio::sync::Notify::new());

    Mock::given(method("GET"))
        .and(path("/community/v1/collections"))
        .respond_with({
            let notify = Arc::clone(&notify);
            move |_request: &Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(300))
                    .set_body_json(json!({ "collections": [{ "id": "private-a" }] }))
            }
        })
        .expect(1)
        .mount(&server)
        .await;

    let worker = service.clone();
    let task = tokio::spawn(async move {
        worker
            .list_collections(&PageOptions::from_value(&json!({ "mine": true })))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), notify.notified())
        .await
        .expect("requisição não chegou ao mock");
    service.set_active(Some("u2")).unwrap();

    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error, SocialError::AccountSwitched);
    assert_eq!(
        error.to_community_envelope(),
        json!({ "ok": false, "error": "conta_trocada", "code": "conta_trocada" })
    );
    assert!(!dir
        .join("contas")
        .join("u2")
        .join(COMMUNITY_CACHE_FILE)
        .exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn canonical_pair_is_stable_for_the_frozen_client() {
    let first = canonical_pair("b", "a");
    let second = canonical_pair("a", "b");
    assert_eq!(first, second);
    assert_eq!(first.user_a, "a");
    assert_eq!(first.user_b, "b");
    assert_eq!(canonical_pair("x", "x").user_a, "x");
}
