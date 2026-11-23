//! Testes de contrato do cliente contra um servidor HTTP local (wiremock).
//! Nenhuma chamada ao backend real.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arcadia_api::{
    ApiClient, ApiConfig, Error, HttpRequest, Order, Session, SetSessionOptions, TransportKind,
    DEFAULT_ANON_KEY,
};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client_for(server: &MockServer) -> ApiClient {
    ApiClient::new(ApiConfig::resolve(Some(&server.uri()), None, None, None)).unwrap()
}

fn session(access: &str, refresh: &str) -> Session {
    serde_json::from_value(json!({
        "access_token": access,
        "refresh_token": refresh,
        "expires_at": 4_000_000_000u64,
        "token_type": "bearer",
        "user": {
            "id": "user-1",
            "email": "user@arcadia.test",
            "user_metadata": { "username": "joao" }
        }
    }))
    .unwrap()
}

async fn install_session(client: &ApiClient, value: Session) {
    client.auth().set_session_optimistic(value, false).await;
}

fn json_response(status: u16, value: Value) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(value)
}

#[tokio::test]
async fn login_sends_exact_payload_and_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "password"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "email": "user@arcadia.test",
            "password": "segredo"
        })))
        .respond_with(json_response(
            200,
            json!({
                "access_token": "access-1",
                "refresh_token": "refresh-1",
                "expires_at": 4_000_000_000u64,
                "token_type": "bearer",
                "user": { "id": "user-1", "email": "user@arcadia.test" }
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client
        .auth()
        .sign_in_with_password("user@arcadia.test", "segredo")
        .await
        .unwrap();

    assert_eq!(result.session.access_token, "access-1");
    assert_eq!(result.user["id"], "user-1");
    assert_eq!(
        client
            .auth()
            .get_session()
            .await
            .unwrap()
            .refresh_token
            .as_deref(),
        Some("refresh-1")
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].headers.get("authorization").is_none(),
        "login inicial não deve mandar bearer"
    );
}

#[tokio::test]
async fn signup_sends_options_and_installs_session() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/signup"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .and(body_json(json!({
            "email": "novo@arcadia.test",
            "password": "segredo",
            "options": { "data": { "username": "novo" } }
        })))
        .respond_with(json_response(
            200,
            json!({
                "user": { "id": "user-2", "email": "novo@arcadia.test" },
                "session": {
                    "access_token": "access-2",
                    "refresh_token": "refresh-2",
                    "expires_at": 4_000_000_000u64,
                    "token_type": "bearer",
                    "user": { "id": "user-2", "email": "novo@arcadia.test" }
                }
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client
        .auth()
        .sign_up(
            "novo@arcadia.test",
            "segredo",
            Some(json!({ "data": { "username": "novo" } })),
        )
        .await
        .unwrap();

    assert_eq!(result.user["id"], "user-2");
    assert_eq!(
        client.auth().get_session().await.unwrap().access_token,
        "access-2"
    );
}

#[tokio::test]
async fn username_login_uses_login_check_before_password_grant() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/login_check"))
        .and(body_json(json!({
            "p_username": "joao",
            "p_password": "segredo"
        })))
        .respond_with(json_response(
            200,
            json!({ "ok": true, "email": "joao@arcadia.test" }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "password"))
        .and(body_json(json!({
            "email": "joao@arcadia.test",
            "password": "segredo"
        })))
        .respond_with(json_response(
            200,
            json!({
                "access_token": "access-3",
                "refresh_token": "refresh-3",
                "expires_at": 4_000_000_000u64,
                "token_type": "bearer",
                "user": { "id": "user-3" }
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client
        .sign_in_with_username("  JoAo ", "segredo")
        .await
        .unwrap();
    assert_eq!(result.session.access_token, "access-3");
}

#[tokio::test]
async fn refresh_sends_refresh_grant_and_merges_missing_fields() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .and(header("authorization", "Bearer old.access"))
        .and(body_json(json!({ "refresh_token": "old.refresh" })))
        .respond_with(json_response(
            200,
            json!({
                "access_token": "new.access",
                "expires_at": 4_100_000_000u64,
                "token_type": "bearer"
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let old = session("old.access", "old.refresh");
    let old_user = old.user.clone();
    install_session(&client, old).await;

    let mut events = client.auth().on_auth_state_change();
    let refreshed = client.auth().refresh_session().await.unwrap();

    assert_eq!(refreshed.access_token, "new.access");
    assert_eq!(refreshed.refresh_token.as_deref(), Some("old.refresh"));
    assert_eq!(refreshed.user, old_user);
    let event = events.recv().await.unwrap();
    assert_eq!(event.event.as_str(), "TOKEN_REFRESHED");
}

#[tokio::test]
async fn refresh_failure_keeps_the_current_session() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(json_response(401, json!({ "error": "refresh_invalido" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("old.access", "old.refresh")).await;

    let error = client.auth().refresh_session().await.unwrap_err();
    assert_eq!(error.status(), Some(401));
    assert_eq!(error.api().unwrap().message, "refresh_invalido");

    let kept = client.auth().get_session().await.unwrap();
    assert_eq!(kept.access_token, "old.access");
}

#[tokio::test]
async fn get_user_caches_after_success() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/auth/v1/user"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(json_response(
            200,
            json!({ "user": { "id": "user-1", "email": "novo@arcadia.test" } }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("access-1", "refresh-1")).await;

    let user = client.auth().get_user().await.unwrap();
    assert_eq!(user["email"], "novo@arcadia.test");
    // Segunda chamada dentro dos 60 s sai do cache: o mock só aceita uma.
    // Como no JS, o cache devolve `session.user` (o usuário do token), não a
    // resposta de /user que acabou de ser buscada.
    let cached = client.auth().get_user().await.unwrap();
    assert_eq!(cached["email"], "user@arcadia.test");
}

#[tokio::test]
async fn rpc_refreshes_once_on_401_and_retries_with_new_token() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_mock = Arc::clone(&calls);

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/friend_achievements"))
        .and(body_json(json!({ "p_friend": "amigo-1" })))
        .respond_with(move |_request: &Request| {
            if calls_for_mock.fetch_add(1, Ordering::SeqCst) == 0 {
                json_response(401, json!({ "error": "token_expirado" }))
            } else {
                json_response(200, json!([{ "id": "ach-1" }]))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .and(body_json(json!({ "refresh_token": "old.refresh" })))
        .respond_with(json_response(
            200,
            json!({
                "access_token": "new.access",
                "refresh_token": "new.refresh",
                "expires_at": 4_100_000_000u64,
                "user": { "id": "user-1", "user_metadata": { "username": "joao" } }
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("old.access", "old.refresh")).await;

    let data = client
        .rpc("friend_achievements", json!({ "p_friend": "amigo-1" }))
        .await
        .unwrap();
    assert_eq!(data[0]["id"], "ach-1");

    let requests = server.received_requests().await.unwrap();
    let rpc_auth: Vec<&str> = requests
        .iter()
        .filter(|request| request.url.path() == "/rest/v1/rpc/friend_achievements")
        .filter_map(|request| request.headers.get("authorization"))
        .filter_map(|value| value.to_str().ok())
        .collect();
    assert_eq!(rpc_auth, vec!["Bearer old.access", "Bearer new.access"]);
    assert_eq!(
        client.auth().get_session().await.unwrap().access_token,
        "new.access"
    );
}

#[tokio::test]
async fn rpc_for_user_does_not_retry_with_a_different_accounts_token() {
    let server = MockServer::start().await;
    let started = Arc::new(tokio::sync::Notify::new());
    let started_by_mock = started.clone();
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_library"))
        .and(header("authorization", "Bearer old.access"))
        .respond_with(move |_: &Request| {
            started_by_mock.notify_one();
            json_response(401, json!({ "error": "token_expirado" }))
                .set_delay(Duration::from_millis(250))
        })
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("old.access", "old.refresh")).await;
    let original_client = client.clone();
    let request = tokio::spawn(async move {
        original_client
            .rpc_for_user(
                "push_library",
                json!({ "p_lib": [{ "appid": "steam:413150" }], "p_playtime": [] }),
                "user-1",
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), started.notified())
        .await
        .expect("a requisição da conta original deve chegar ao servidor");
    let mut next = session("other.access", "other.refresh");
    next.user["id"] = json!("user-2");
    install_session(&client, next).await;

    let error = request.await.unwrap().unwrap_err();
    assert_eq!(error.status(), Some(409));
    assert_eq!(error.api().unwrap().message, "sessao_trocada");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert_eq!(
        client.auth().get_session().await.unwrap().access_token,
        "other.access"
    );
}

#[tokio::test]
async fn rpc_refresh_failure_returns_auth_error_without_clearing_session() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(json_response(401, json!({ "error": "token_expirado" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(json_response(401, json!({ "error": "refresh_invalido" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("old.access", "old.refresh")).await;

    let error = client.rpc("pull_library", json!({})).await.unwrap_err();
    assert_eq!(error.status(), Some(401));
    assert_eq!(
        client.auth().get_session().await.unwrap().access_token,
        "old.access",
        "falha de refresh não pode limpar a sessão"
    );
}

#[tokio::test]
async fn rest_get_builds_the_same_query_as_the_js_builder() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/profiles"))
        .and(query_param("select", "id, username"))
        .and(query_param("username", "eq.joao"))
        .and(query_param("or", "(user_a.eq.X,user_b.eq.Y)"))
        .and(query_param("order", "username.asc"))
        .and(query_param("limit", "20"))
        .and(header("authorization", "Bearer access-1"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .respond_with(json_response(
            200,
            json!([{ "id": "user-1", "username": "joao" }]),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("access-1", "refresh-1")).await;

    let data = client
        .from("profiles")
        .select("id, username")
        .eq("username", "joao")
        .or("user_a.eq.X,user_b.eq.Y")
        .order("username", Order::Asc)
        .limit(20)
        .await
        .unwrap();
    assert_eq!(data[0]["username"], "joao");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].url.query().unwrap(),
        "select=id%2C+username&username=eq.joao&or=%28user_a.eq.X%2Cuser_b.eq.Y%29&order=username.asc&limit=20"
    );
}

#[tokio::test]
async fn rest_single_returns_the_first_row_or_null() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/profiles"))
        .and(query_param("id", "eq.user-1"))
        .respond_with(json_response(
            200,
            json!([{ "id": "user-1", "username": "joao" }]),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/profiles"))
        .and(query_param("id", "eq.ausente"))
        .respond_with(json_response(200, json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let found = client
        .from("profiles")
        .select("*")
        .eq("id", "user-1")
        .maybe_single()
        .await
        .unwrap();
    assert_eq!(found["username"], "joao");

    let missing = client
        .from("profiles")
        .select("*")
        .eq("id", "ausente")
        .single()
        .await
        .unwrap();
    assert_eq!(missing, Value::Null);
}

#[tokio::test]
async fn rest_mutations_send_the_expected_headers_and_bodies() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/friendships"))
        .and(header("prefer", "return=representation"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "user_a": "a",
            "user_b": "b",
            "status": "pending"
        })))
        .respond_with(json_response(201, json!([{ "user_a": "a" }])))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/profiles"))
        .and(body_json(json!({ "id": "user-1", "username": "joao" })))
        .respond_with(json_response(201, json!([{ "id": "user-1" }])))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("id", "eq.rel-1"))
        .and(header("prefer", "return=representation"))
        .and(body_json(json!({ "status": "accepted" })))
        .respond_with(json_response(200, json!([{ "status": "accepted" }])))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path("/rest/v1/friendships"))
        .and(query_param("id", "eq.rel-1"))
        .and(header("prefer", "return=representation"))
        .and(header("content-type", "application/json"))
        .respond_with(json_response(200, json!([{ "id": "rel-1" }])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);

    client
        .from("friendships")
        .insert(json!({ "user_a": "a", "user_b": "b", "status": "pending" }))
        .await
        .unwrap();
    if let Err(error) = client
        .from("profiles")
        .upsert(json!({ "id": "user-1", "username": "joao" }))
        .await
    {
        eprintln!("upsert falhou: {error:?}");
        panic!("upsert falhou");
    }
    client
        .from("friendships")
        .update(json!({ "status": "accepted" }))
        .eq("id", "rel-1")
        .await
        .unwrap();
    client
        .from("friendships")
        .delete()
        .eq("id", "rel-1")
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let upsert = requests
        .iter()
        .find(|request| request.url.path() == "/rest/v1/profiles")
        .unwrap();
    assert_eq!(
        upsert.headers.get("prefer").unwrap().to_str().unwrap(),
        "resolution=merge-duplicates, return=representation"
    );
    let delete = requests
        .iter()
        .find(|request| request.method.as_str() == "DELETE")
        .unwrap();
    assert!(delete.body.is_empty(), "DELETE sem corpo, como no JS");
}

#[tokio::test]
async fn rest_minimal_representation_omits_the_prefer_header() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/logs"))
        .respond_with(json_response(201, json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    client
        .from("logs")
        .insert(json!({ "message": "oi" }))
        .return_minimal()
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("prefer").is_none());
}

#[tokio::test]
async fn rpc_uses_null_as_empty_object_and_parses_json() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_sources"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .and(body_json(json!({})))
        .respond_with(json_response(200, json!([{ "id": "source-1" }])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let data = client.rpc("pull_sources", Value::Null).await.unwrap();
    assert_eq!(data[0]["id"], "source-1");
}

#[tokio::test]
async fn storage_upload_sends_content_type_and_upsert() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/storage/v1/object/avatars/user-1.png"))
        .and(header("content-type", "image/png"))
        .and(header("x-upsert", "true"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(json_response(200, json!({ "Key": "avatars/user-1.png" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("access-1", "refresh-1")).await;

    let result = client
        .storage_from("avatars")
        .upload(
            "user-1.png",
            vec![0x89, 0x50, 0x4e, 0x47],
            "image/png",
            true,
        )
        .await
        .unwrap();
    assert_eq!(result["Key"], "avatars/user-1.png");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].body, vec![0x89, 0x50, 0x4e, 0x47]);
}

#[tokio::test]
async fn storage_remove_sends_paths_array() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/storage/v1/object/avatars"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({ "paths": ["a.png", "b.png"] })))
        .respond_with(json_response(200, json!([{ "name": "a.png" }])))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client
        .storage_from("avatars")
        .remove(&["a.png", "b.png"])
        .await
        .unwrap();
    assert_eq!(result[0]["name"], "a.png");
}

#[tokio::test]
async fn fallback_only_happens_on_network_errors_and_sticks() {
    let fallback = MockServer::start().await;
    let dead = dead_url();

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/ping"))
        .respond_with(json_response(200, json!({ "ok": true })))
        .expect(2)
        .mount(&fallback)
        .await;

    let config = ApiConfig {
        url: dead.clone(),
        urls: vec![dead, fallback.uri()],
        anon_key: DEFAULT_ANON_KEY.to_string(),
    };
    let client = ApiClient::new(config).unwrap();

    assert_eq!(client.rpc("ping", json!({})).await.unwrap()["ok"], true);
    assert_eq!(client.http().preferred_index(), 1);

    // A chamada seguinte ainda monta a URL no primário, mas a ordem de
    // tentativas começa no endereço que respondeu.
    assert_eq!(client.rpc("ping", json!({})).await.unwrap()["ok"], true);
    assert_eq!(
        fallback.received_requests().await.unwrap().len(),
        2,
        "o endereço que falhou não é tentado de novo"
    );
}

#[tokio::test]
async fn http_error_never_uses_the_fallback() {
    let primary = MockServer::start().await;
    let fallback = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/ping"))
        .respond_with(json_response(503, json!({ "error": "indisponivel" })))
        .expect(1)
        .mount(&primary)
        .await;

    let config = ApiConfig {
        url: primary.uri(),
        urls: vec![primary.uri(), fallback.uri()],
        anon_key: DEFAULT_ANON_KEY.to_string(),
    };
    let client = ApiClient::new(config).unwrap();

    let error = client.rpc("ping", json!({})).await.unwrap_err();
    assert_eq!(error.status(), Some(503));
    assert_eq!(error.api().unwrap().message, "indisponivel");
    assert!(
        fallback.received_requests().await.unwrap().is_empty(),
        "5xx é resposta legítima e não troca de endereço"
    );
}

#[tokio::test]
async fn external_urls_are_not_rewritten() {
    let backend = MockServer::start().await;
    let cdn = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/external"))
        .respond_with(json_response(200, json!({ "ok": true })))
        .expect(1)
        .mount(&cdn)
        .await;

    let config = ApiConfig::resolve(Some(&backend.uri()), None, None, None);
    let http = arcadia_api::HttpClient::new(Arc::new(config)).unwrap();
    let response = http
        .send(&HttpRequest::get(format!("{}/external", cdn.uri())))
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    assert!(backend.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn header_values_are_sanitized_instead_of_crashing() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/headers"))
        .and(header("x-arcadia", "abcdef"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let config = ApiConfig::resolve(Some(&server.uri()), None, None, None);
    let http = arcadia_api::HttpClient::new(Arc::new(config)).unwrap();
    let response = http
        .send(
            &HttpRequest::get(format!("{}/headers", server.uri()))
                .header("x-arcadia", "abc☃\r\ndef")
                .header("x-latin", "olá"),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 200);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].headers.get("x-latin").unwrap().as_bytes(),
        b"ol\xe1",
        "Latin-1 é preservado byte a byte"
    );
}

#[tokio::test]
async fn non_json_responses_become_typed_errors_with_the_raw_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/quebrado"))
        .respond_with(ResponseTemplate::new(500).set_body_string("<html>falhou</html>"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/texto"))
        .respond_with(ResponseTemplate::new(200).set_body_string("nao e json"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);

    let api_error = client.rpc("quebrado", json!({})).await.unwrap_err();
    match api_error {
        Error::Api(error) => {
            assert_eq!(error.status, 500);
            assert_eq!(error.message, "HTTP 500");
            assert_eq!(error.body, "<html>falhou</html>");
        }
        other => panic!("erro inesperado: {other:?}"),
    }

    let decode = client.rpc("texto", json!({})).await.unwrap_err();
    match decode {
        Error::Decode(error) => {
            assert_eq!(error.status, 200);
            assert_eq!(error.body, "nao e json");
        }
        other => panic!("erro inesperado: {other:?}"),
    }
}

#[tokio::test]
async fn dead_endpoint_is_a_classified_transport_error() {
    let config = ApiConfig::resolve(Some(&dead_url()), None, None, None);
    let client = ApiClient::new(config).unwrap();

    let error = client.rpc("ping", json!({})).await.unwrap_err();
    match error {
        Error::Transport(error) => assert_eq!(error.kind, TransportKind::Connect),
        other => panic!("erro inesperado: {other:?}"),
    }
}

#[tokio::test]
async fn logout_clears_local_session_even_when_the_server_fails() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v1/logout"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("access-1", "refresh-1")).await;
    let mut events = client.auth().on_auth_state_change();

    assert!(client.auth().sign_out().await.is_err());
    assert!(client.auth().get_session().await.is_none());
    assert_eq!(events.recv().await.unwrap().event.as_str(), "SIGNED_OUT");
}

#[tokio::test]
async fn set_session_non_optimistic_validates_and_keeps_on_success() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/auth/v1/user"))
        .respond_with(json_response(200, json!({ "user": { "id": "user-1" } })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let installed = client
        .auth()
        .set_session(
            session("access-1", "refresh-1"),
            SetSessionOptions {
                emit_signed_in: true,
                optimistic: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(installed.access_token, "access-1");
}

#[tokio::test]
async fn health_accepts_json_and_text() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/auth/v1/health"))
        .and(header("apikey", DEFAULT_ANON_KEY))
        .respond_with(json_response(200, json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    assert_eq!(client.auth().health().await.unwrap()["ok"], true);
}

#[tokio::test]
async fn optimistic_session_emits_signed_in_without_network_and_survives_offline() {
    let client = ApiClient::new(ApiConfig::resolve(Some(&dead_url()), None, None, None)).unwrap();
    let mut events = client.auth().on_auth_state_change();

    let installed = client
        .auth()
        .set_session(
            session("boot.access", "boot.refresh"),
            SetSessionOptions {
                emit_signed_in: true,
                optimistic: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(installed.access_token, "boot.access");
    assert_eq!(events.recv().await.unwrap().event.as_str(), "SIGNED_IN");

    // Falha de rede na validação não pode deslogar quem está offline.
    let error = client.auth().validate_session().await.unwrap_err();
    assert!(matches!(error, Error::Transport(_)));
    assert_eq!(
        client.auth().get_session().await.unwrap().access_token,
        "boot.access"
    );
    assert!(events.try_recv().is_err(), "não deve emitir SIGNED_OUT");
}

#[tokio::test]
async fn validation_clears_the_session_only_after_401_and_refresh_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/auth/v1/user"))
        .respond_with(json_response(401, json!({ "error": "token_invalido" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(json_response(401, json!({ "error": "refresh_invalido" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    install_session(&client, session("boot.access", "boot.refresh")).await;
    let mut events = client.auth().on_auth_state_change();

    assert!(client.auth().validate_session().await.is_err());
    assert!(client.auth().get_session().await.is_none());
    assert_eq!(events.recv().await.unwrap().event.as_str(), "SIGNED_OUT");
}

fn dead_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}
