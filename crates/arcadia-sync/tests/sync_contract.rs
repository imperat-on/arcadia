//! Testes de contrato do sync contra um servidor HTTP local (wiremock).
//! Nenhuma chamada ao backend real; arquivos ficam em diretórios temporários.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arcadia_api::{ApiClient, ApiConfig, Session};
use arcadia_store::accounts::AccountScope;
use arcadia_sync::{
    achievements, library, playtime, sources, SyncEngine, SyncError, SyncEvent, SyncState,
};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "arcadia-sync-it-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scope_for(dir: &std::path::Path) -> AccountScope {
    AccountScope::with_active(dir, Some("u1"))
}

fn client_for(server: &MockServer) -> ApiClient {
    ApiClient::new(ApiConfig::resolve(Some(&server.uri()), None, None, None)).unwrap()
}

fn session() -> Session {
    serde_json::from_value(json!({
        "access_token": "test-token",
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
    client.auth().set_session_optimistic(session(), false).await;
}

fn write_json(path: &std::path::Path, value: Value) {
    std::fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn request_json(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

fn null_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(Value::Null)
}

fn auth_header() -> wiremock::matchers::HeaderExactMatcher {
    header("authorization", "Bearer test-token")
}

#[tokio::test]
async fn push_library_sends_exact_payload_and_updates_watermarks() {
    let server = MockServer::start().await;
    let dir = temp_dir("push-library");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("custom_games.json"),
        json!([{ "id": "custom:1", "title": "Jogo", "launcher": "custom" }]),
    );
    write_json(
        &scope.account_file("pending_games.json"),
        json!([{ "id": "steam:10", "title": "Steam Ten", "launcher": "steam" }]),
    );
    write_json(
        &scope.account_file("overrides.json"),
        json!({ "custom:1": { "playtime_added_minutes": 75 } }),
    );
    write_json(
        &scope.account_file("sync_state.json"),
        json!({
            "libPush": {
                "custom:1": { "title": "Old", "platform": "windows" },
                "steam:77": { "title": "Gone", "platform": "windows" }
            },
            "playtimePush": { "custom:1": 20 }
        }),
    );
    arcadia_store::owned::write_owned(&scope, &[json!("steam:10")]).unwrap();

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_library"))
        .and(auth_header())
        .and(body_json(json!({
            "p_lib": [
                { "appid": "custom:1", "title": "Jogo", "platform": "windows" },
                { "appid": "steam:10", "title": "Steam Ten", "platform": "windows" },
                { "appid": "steam:77", "removed": true }
            ],
            "p_playtime": [
                { "appid": "custom:1", "minutes": 55 }
            ]
        })))
        .respond_with(null_response())
        .expect(1)
        .mount(&server)
        .await;

    let outcome = library::push(&client, &scope).await.unwrap();
    assert_eq!(outcome.pushed, 3);
    assert_eq!(outcome.playtime, 1);

    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["libPush"]["custom:1"]["title"], "Jogo");
    assert_eq!(state["libPush"]["steam:10"]["title"], "Steam Ten");
    assert!(state["libPush"].get("steam:77").is_none());
    assert_eq!(state["playtimePush"]["custom:1"], 75);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn pull_library_uses_exact_rpcs_and_applies_stubs_ownership_and_playtime() {
    let server = MockServer::start().await;
    let dir = temp_dir("pull-library");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("custom_games.json"),
        json!([{ "id": "custom:local", "title": "Local", "launcher": "custom" }]),
    );
    write_json(
        &scope.account_file("overrides.json"),
        json!({ "custom:local": { "playtime_added_minutes": 20 } }),
    );
    write_json(
        &scope.account_file("sync_state.json"),
        json!({ "libPush": { "steam:77": { "title": "Old", "platform": "windows" } } }),
    );
    let stardew_stub = json!({
        "id": "steam:413150", "title": "Stardew Valley", "launcher": "steam", "pendente": true
    });
    write_json(
        &scope.account_file("pending_games.json"),
        json!([stardew_stub]),
    );
    arcadia_store::owned::write_owned(&scope, &[json!("steam:77"), json!("steam:413150")]).unwrap();

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .and(auth_header())
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "appid": "steam:42", "title": "Steam Game", "platform": "windows", "minutes": 120 },
            { "appid": "custom:remote", "title": "Remoto", "platform": "linux", "minutes": 30 }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "appid": "42", "steamid": "765", "minutes": 500 }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = library::pull(&client, &scope).await.unwrap();
    assert_eq!(outcome.pulled, 2);
    assert_eq!(outcome.steam_rows, 1);
    assert!(outcome.changed);

    let custom = read_json(&scope.account_file("custom_games.json"));
    let remote = custom
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["id"] == "custom:remote")
        .unwrap();
    assert_eq!(remote["title"], "Remoto");
    assert_eq!(remote["platform"], "linux");
    assert_eq!(remote["launcher"], "custom");
    assert_eq!(remote["exe"], "");
    assert_eq!(remote["installed"], false);

    let pending = read_json(&scope.account_file("pending_games.json"));
    let stub = pending
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["id"] == "steam:42")
        .unwrap();
    assert_eq!(stub["title"], "Steam Game");
    assert_eq!(stub["launcher"], "steam");
    assert_eq!(stub["pendente"], true);
    assert!(pending.as_array().unwrap().contains(&stardew_stub));

    let owned = read_json(&scope.account_file("owned_games.json"));
    assert!(owned.as_array().unwrap().contains(&json!("steam:42")));
    assert!(owned.as_array().unwrap().contains(&json!("custom:remote")));
    assert!(owned.as_array().unwrap().contains(&json!("steam:413150")));
    assert!(!owned.as_array().unwrap().contains(&json!("steam:77")));

    let overrides = read_json(&scope.account_file("overrides.json"));
    assert_eq!(overrides["custom:local"]["playtime_added_minutes"], 20);
    assert_eq!(overrides["custom:remote"]["playtime_added_minutes"], 30);
    assert_eq!(overrides["steam:42"]["playtime_added_minutes"], 120);
    assert!(!outcome.playtime_needs_push);

    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["steamPlaytime"].as_array().unwrap().len(), 1);
    assert_eq!(state["playtimePush"]["custom:remote"], 30);
    assert_eq!(state["playtimePush"]["steam:42"], 120);
    assert_eq!(state["libPush"]["custom:remote"]["title"], "Remoto");
    assert!(state["libPush"].get("steam:413150").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn startup_retries_unsent_steam_addition_and_acknowledges_it_once() {
    let server = MockServer::start().await;
    let dir = temp_dir("retry-steam-addition");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;
    let engine = SyncEngine::new(client, scope.clone());
    let stardew = json!({
        "appid": "steam:413150", "title": "Stardew Valley", "platform": "windows"
    });
    {
        let _guard = library::lock_local_library();
        library::record_steam_change(&scope.account_dir(), "413150", Some("Stardew Valley"))
            .unwrap();
        write_json(
            &scope.account_file("pending_games.json"),
            json!([{ "id": "steam:413150", "title": "Stardew Valley", "launcher": "steam" }]),
        );
        arcadia_store::owned::write_owned(&scope, &[json!("steam:413150")]).unwrap();
    }

    let payload = json!({ "p_lib": [stardew], "p_playtime": [] });
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_library"))
        .and(auth_header())
        .and(body_json(payload.clone()))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    assert!(engine.send_library_changes().await.is_err());
    server.verify().await;
    server.reset().await;
    drop(engine);

    let saved = Arc::new(AtomicBool::new(false));
    let saved_by_push = saved.clone();
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_library"))
        .and(auth_header())
        .and(body_json(payload))
        .respond_with(move |_: &Request| {
            saved_by_push.store(true, Ordering::Relaxed);
            null_response()
        })
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(move |_: &Request| {
            let mut rows = vec![json!({
                "appid": "steam:42", "title": "Steam Game", "platform": "windows"
            })];
            if saved.load(Ordering::Relaxed) {
                rows.push(stardew.clone());
            }
            ResponseTemplate::new(200).set_body_json(rows)
        })
        .mount(&server)
        .await;
    for rpc in ["pull_steam_playtime", "pull_achievements", "pull_sources"] {
        Mock::given(method("POST"))
            .and(path(format!("/rest/v1/rpc/{rpc}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
    }

    let client = client_for(&server);
    install_session(&client).await;
    let restarted = SyncEngine::new(client.clone(), scope.clone());
    restarted.reconcile().await.unwrap();
    assert!(arcadia_store::owned::read_owned(&scope)
        .unwrap()
        .contains(&json!("steam:413150")));
    assert_eq!(
        read_json(&scope.account_file("sync_state.json"))["libPush"]["steam:413150"]["title"],
        "Stardew Valley"
    );
    restarted.send_library_changes().await.unwrap();

    let other_device = scope_for(&dir.join("other-device"));
    arcadia_store::owned::write_owned(&other_device, &[]).unwrap();
    library::pull(&client, &other_device).await.unwrap();
    assert!(arcadia_store::owned::read_owned(&other_device)
        .unwrap()
        .contains(&json!("steam:413150")));
    server.verify().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn queued_steam_removal_blocks_a_stale_pull_and_reaches_the_server() {
    let server = MockServer::start().await;
    let dir = temp_dir("steam-removal");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;
    {
        let _guard = library::lock_local_library();
        library::record_steam_change(&scope.account_dir(), "413150", None).unwrap();
        arcadia_store::owned::write_owned(&scope, &[]).unwrap();
        write_json(&scope.account_file("pending_games.json"), json!([]));
    }
    write_json(
        &scope.account_file("sync_state.json"),
        json!({ "libPush": { "steam:413150": { "title": "Stardew Valley", "platform": "windows" } } }),
    );

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "appid": "steam:413150", "title": "Stardew Valley", "platform": "windows" }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_library"))
        .and(auth_header())
        .and(body_json(json!({
            "p_lib": [{ "appid": "steam:413150", "removed": true }], "p_playtime": []
        })))
        .respond_with(null_response())
        .expect(1)
        .mount(&server)
        .await;

    library::pull(&client, &scope).await.unwrap();
    assert_eq!(arcadia_store::owned::read_owned(&scope), Some(Vec::new()));
    assert_eq!(
        read_json(&scope.account_file("pending_games.json")),
        json!([])
    );

    let restarted = SyncEngine::new(client, scope.clone());
    restarted.send_library_changes().await.unwrap();
    restarted.send_library_changes().await.unwrap();
    assert!(read_json(&scope.account_file("sync_state.json"))["libPush"]
        .get("steam:413150")
        .is_none());
    server.verify().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn queued_steam_changes_are_never_sent_using_another_accounts_session() {
    let server = MockServer::start().await;
    let dir = temp_dir("steam-account-switch");
    let scope = AccountScope::with_active(&dir, Some("u2"));
    let client = client_for(&server);
    install_session(&client).await;
    {
        let _guard = library::lock_local_library();
        library::record_steam_change(&scope.account_dir(), "413150", Some("Stardew Valley"))
            .unwrap();
    }
    let engine = SyncEngine::new(client, scope);

    assert_eq!(
        engine.send_library_changes().await.unwrap_err(),
        SyncError::AccountSwitched
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn empty_pull_requires_two_observations_before_removing_ownership() {
    let server = MockServer::start().await;
    let dir = temp_dir("empty-streak");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    arcadia_store::owned::write_owned(&scope, &[json!("steam:1")]).unwrap();
    write_json(
        &scope.account_file("sync_state.json"),
        json!({ "libPush": { "steam:1": { "title": "Um", "platform": "windows" } } }),
    );

    for attempt in 1..=2 {
        Mock::given(method("POST"))
            .and(path("/rest/v1/rpc/pull_library"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rest/v1/rpc/pull_steam_playtime"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        library::pull(&client, &scope).await.unwrap();
        let owned = read_json(&scope.account_file("owned_games.json"));
        if attempt == 1 {
            assert!(
                owned.as_array().unwrap().contains(&json!("steam:1")),
                "primeiro pull vazio não pode remover posse"
            );
            let state = read_json(&scope.account_file("sync_state.json"));
            assert_eq!(state["emptyPullStreak"], 1);
        } else {
            assert!(
                owned.as_array().unwrap().is_empty(),
                "segundo pull vazio confirma a remoção"
            );
        }
        server.reset().await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn library_conflict_newer_wins_title_quality_and_canonical_tiebreak() {
    let server = MockServer::start().await;
    let dir = temp_dir("conflict");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("custom_games.json"),
        json!([
            { "id": "custom:new", "title": "Old", "platform": "windows", "updated_at": 100 },
            { "id": "custom:quality", "title": "Steam 123", "platform": "windows" },
            { "id": "custom:tie", "title": "Zeta", "platform": "windows" }
        ]),
    );
    arcadia_store::owned::write_owned(&scope, &[]).unwrap();

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "appid": "custom:new", "title": "New", "platform": "linux", "updated_at": 200 },
            { "appid": "custom:quality", "title": "Real Name", "platform": "windows" },
            { "appid": "custom:tie", "title": "Alpha", "platform": "windows" }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    library::pull(&client, &scope).await.unwrap();
    let custom = read_json(&scope.account_file("custom_games.json"));
    let find = |id: &str| {
        custom
            .as_array()
            .unwrap()
            .iter()
            .find(|game| game["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(find("custom:new")["title"], "New");
    assert_eq!(find("custom:new")["platform"], "linux");
    assert_eq!(find("custom:quality")["title"], "Real Name");
    assert_eq!(find("custom:tie")["title"], "Alpha");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn push_achievements_sends_exact_payload_and_keeps_other_queue_domains() {
    let server = MockServer::start().await;
    let dir = temp_dir("push-achievements");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("sync_queue.json"),
        json!([
            { "kind": "source", "source_id": "keep-me" },
            {
                "appid": "steam:1",
                "apiname": "ACH01",
                "unlocked_at": 1_700_000_000,
                "achieved": true,
                "title": "Primeira",
                "percent": 50
            }
        ]),
    );

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/sync_achievements"))
        .and(auth_header())
        .and(body_json(json!({
            "p_items": [{
                "appid": "steam:1",
                "apiname": "ACH01",
                "unlocked_at": 1_700_000_000u64,
                "achieved": true,
                "title": "Primeira",
                "icon": null,
                "percent": 50
            }]
        })))
        .respond_with(null_response())
        .expect(1)
        .mount(&server)
        .await;

    let outcome = achievements::push_queue(&client, &scope).await.unwrap();
    assert_eq!(outcome.pushed, 1);
    assert_eq!(
        read_json(&scope.account_file("sync_queue.json")),
        json!([{ "kind": "source", "source_id": "keep-me" }])
    );
    let state = read_json(&scope.account_file("sync_state.json"));
    assert!(state["lastSyncAt"].as_i64().unwrap() > 0);
    assert_eq!(state["lastError"], Value::Null);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn pull_achievements_uses_cursor_and_earliest_wins() {
    let server = MockServer::start().await;
    let dir = temp_dir("pull-achievements");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("achievements.json"),
        json!({
            "steam:1": {
                "at": 1,
                "items": [{
                    "apiname": "ACH01",
                    "title": "Primeira",
                    "achieved": true,
                    "unlock": 200,
                    "aliases": ["ach_01"],
                    "unknown": "keep"
                }]
            }
        }),
    );
    write_json(
        &scope.account_file("sync_state.json"),
        json!({ "lastPullAt": 1_700_000_000, "future": true }),
    );

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_achievements"))
        .and(auth_header())
        .and(body_json(json!({ "p_since": "2023-11-14T22:13:20.000Z" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "appid": "steam:1",
                "apiname": "ach_01",
                "unlocked_at": 100,
                "title": "Primeira",
                "percent": 10
            },
            {
                "appid": "steam:2",
                "apiname": "ACH02",
                "unlocked_at": 50,
                "title": "Nova",
                "icon": "icone",
                "percent": null
            },
            {
                "appid": "steam:3",
                "apiname": "ACH03",
                "achieved": true,
                "unlocked_at": null,
                "title": "SemTs"
            }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = achievements::pull_delta(&client, &scope).await.unwrap();
    assert_eq!(outcome.pulled, 3);
    assert!(outcome.changed);

    let store = read_json(&scope.account_file("achievements.json"));
    let first = &store["steam:1"]["items"][0];
    assert_eq!(first["achieved"], true);
    assert_eq!(first["unlock"], 100);
    assert_eq!(first["remoteApiname"], "ach_01");
    assert_eq!(first["aliases"], json!(["ach_01"]));
    assert_eq!(first["unknown"], "keep");

    let second = &store["steam:2"]["items"][0];
    assert_eq!(second["unlock"], 50);
    assert_eq!(second["percent"], 100);
    assert_eq!(second["icon"], "icone");

    let third = &store["steam:3"]["items"][0];
    assert_eq!(third["achieved"], true);
    assert_eq!(third["unlock"], 0);

    let state = read_json(&scope.account_file("sync_state.json"));
    assert_ne!(state["lastPullAt"], 1_700_000_000);
    assert!(state["lastPullAt"].as_i64().unwrap() > 1_700_000_000);
    assert_eq!(state["future"], true);
    assert_eq!(state["lastError"], Value::Null);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn push_and_pull_sources_send_exact_fields_and_preserve_registry() {
    let server = MockServer::start().await;
    let dir = temp_dir("sources");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("sources.json"),
        json!([{
            "id": "a",
            "url": "https://example/a.json",
            "name": "A",
            "etag": "W/\"1\"",
            "lastMod": "ontem",
            "count": 7,
            "apiKey": "segredo"
        }]),
    );
    write_json(
        &scope.account_file("sync_state.json"),
        json!({ "srcPush": { "old": { "url": "https://old", "name": "Old" } } }),
    );

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_sources"))
        .and(auth_header())
        .and(body_json(json!({
            "p_sources": [
                { "source_id": "a", "url": "https://example/a.json", "name": "A" },
                { "source_id": "old", "removed": true }
            ]
        })))
        .respond_with(null_response())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_sources"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "source_id": "b", "url": "https://example/b.json", "name": "B" },
            { "source_id": "a", "url": "https://ignored", "name": "Ignorado" }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let pushed = sources::push(&client, &scope).await.unwrap();
    assert_eq!(pushed.pushed, 2);
    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["srcPush"]["a"]["url"], "https://example/a.json");
    assert!(state["srcPush"].get("old").is_none());

    let pulled = sources::pull(&client, &scope).await.unwrap();
    assert_eq!(pulled.pulled, 2);
    assert!(pulled.changed);
    let registry = read_json(&scope.account_file("sources.json"));
    let entries = registry.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let a = entries.iter().find(|entry| entry["id"] == "a").unwrap();
    assert_eq!(a["url"], "https://example/a.json");
    assert_eq!(a["etag"], "W/\"1\"");
    assert_eq!(a["apiKey"], "segredo");
    let b = entries.iter().find(|entry| entry["id"] == "b").unwrap();
    assert_eq!(b["etag"], "");
    assert_eq!(b["lastMod"], "");
    assert_eq!(b["count"], 0);
    assert!(b["addedAt"].as_i64().unwrap() > 0);
    // Fonte vinda do servidor já entra no watermark: removê-la localmente
    // precisa gerar tombstone no próximo push.
    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["srcPush"]["b"]["url"], "https://example/b.json");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn library_and_playtime_chunk_at_one_thousand_items() {
    let server = MockServer::start().await;
    let dir = temp_dir("chunks");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    let games: Vec<Value> = (0..1_005)
        .map(|index| json!({ "id": format!("custom:{index}"), "title": format!("Game {index}"), "launcher": "custom" }))
        .collect();
    let overrides: serde_json::Map<String, Value> = (0..1_005)
        .map(|index| {
            (
                format!("custom:{index}"),
                json!({ "playtime_added_minutes": index + 1 }),
            )
        })
        .collect();
    write_json(
        &scope.account_file("custom_games.json"),
        Value::Array(games),
    );
    write_json(
        &scope.account_file("overrides.json"),
        Value::Object(overrides),
    );
    arcadia_store::owned::write_owned(&scope, &[]).unwrap();

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_library"))
        .respond_with(null_response())
        .mount(&server)
        .await;

    let outcome = library::push(&client, &scope).await.unwrap();
    assert_eq!(outcome.pushed, 1_005);
    assert_eq!(outcome.playtime, 1_005);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let first = request_json(&requests[0]);
    let second = request_json(&requests[1]);
    assert_eq!(first["p_lib"].as_array().unwrap().len(), 1_000);
    assert_eq!(first["p_playtime"].as_array().unwrap().len(), 1_000);
    assert_eq!(second["p_lib"].as_array().unwrap().len(), 5);
    assert_eq!(second["p_playtime"].as_array().unwrap().len(), 5);

    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["libPush"].as_object().unwrap().len(), 1_005);
    assert_eq!(state["playtimePush"].as_object().unwrap().len(), 1_005);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn achievements_queue_and_sources_chunk_at_one_thousand_items() {
    let server = MockServer::start().await;
    let dir = temp_dir("chunks-2");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    let queue: Vec<Value> = (0..1_001)
        .map(|index| {
            json!({
                "appid": "steam:1",
                "apiname": format!("ACH{index}"),
                "unlocked_at": 1_700_000_000 + index,
                "achieved": true
            })
        })
        .collect();
    write_json(&scope.account_file("sync_queue.json"), Value::Array(queue));
    let registry: Vec<Value> = (0..1_001)
        .map(|index| {
            json!({
                "id": format!("src{index}"),
                "url": format!("https://example/{index}.json"),
                "name": format!("Fonte {index}")
            })
        })
        .collect();
    write_json(&scope.account_file("sources.json"), Value::Array(registry));

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/sync_achievements"))
        .respond_with(null_response())
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_sources"))
        .respond_with(null_response())
        .mount(&server)
        .await;

    let achievements_outcome = achievements::push_queue(&client, &scope).await.unwrap();
    assert_eq!(achievements_outcome.pushed, 1_001);
    let sources_outcome = sources::push(&client, &scope).await.unwrap();
    assert_eq!(sources_outcome.pushed, 1_001);

    let requests = server.received_requests().await.unwrap();
    let achievement_bodies: Vec<Value> = requests
        .iter()
        .filter(|request| request.url.path().ends_with("sync_achievements"))
        .map(request_json)
        .collect();
    assert_eq!(achievement_bodies.len(), 2);
    assert_eq!(
        achievement_bodies[0]["p_items"].as_array().unwrap().len(),
        1_000
    );
    assert_eq!(
        achievement_bodies[1]["p_items"].as_array().unwrap().len(),
        1
    );
    let source_bodies: Vec<Value> = requests
        .iter()
        .filter(|request| request.url.path().ends_with("push_sources"))
        .map(request_json)
        .collect();
    assert_eq!(source_bodies.len(), 2);
    assert_eq!(
        source_bodies[0]["p_sources"].as_array().unwrap().len(),
        1_000
    );
    assert_eq!(source_bodies[1]["p_sources"].as_array().unwrap().len(), 1);

    assert_eq!(read_json(&scope.account_file("sync_queue.json")), json!([]));
    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["srcPush"].as_object().unwrap().len(), 1_001);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn push_steam_playtime_sends_absolute_totals_and_sanitizes() {
    let server = MockServer::start().await;
    let dir = temp_dir("steam-push");
    let _scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_steam_playtime"))
        .and(auth_header())
        .and(body_json(json!({
            "p_items": [
                { "appid": "440", "steamid": "765", "minutes": 120, "persona": "Zes" },
                { "appid": "10", "steamid": "765", "minutes": 30, "persona": "" }
            ]
        })))
        .respond_with(null_response())
        .expect(1)
        .mount(&server)
        .await;

    let outcome = playtime::push_steam(
        &client,
        &[
            json!({ "appid": "440", "steamid": "765", "minutes": 120, "persona": "Zes" }),
            json!({ "appid": "10", "steamid": 765, "minutes": "30" }),
            json!({ "appid": "440", "minutes": 1 }),
            json!({ "appid": "440", "steamid": "765", "minutes": 0 }),
        ],
    )
    .await
    .unwrap();
    assert_eq!(outcome.pushed, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn pull_only_blocks_every_push_and_still_pulls() {
    let server = MockServer::start().await;
    let dir = temp_dir("pull-only");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("custom_games.json"),
        json!([{ "id": "custom:1", "title": "Local", "launcher": "custom" }]),
    );
    write_json(
        &scope.account_file("sync_queue.json"),
        json!([{ "appid": "steam:1", "apiname": "ACH01", "unlocked_at": 1, "achieved": true }]),
    );

    let events: Arc<Mutex<Vec<SyncEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let engine = SyncEngine::new(client.clone(), scope.clone())
        .with_event_listener(move |event| sink.lock().unwrap().push(event.clone()));
    assert!(engine.pull_only());

    let error = engine.push_library().await.unwrap_err();
    assert_eq!(error, SyncError::PullOnly);
    let error = engine
        .push_steam_playtime(&[json!({
            "appid": "440",
            "steamid": "765",
            "minutes": 10
        })])
        .await
        .unwrap_err();
    assert_eq!(error, SyncError::PullOnly);
    assert!(server.received_requests().await.unwrap().is_empty());

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_achievements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_sources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let report = engine.sync_now().await.unwrap();
    assert_eq!(report.library_pushed, 0);
    assert_eq!(report.achievements_pushed, 0);
    assert_eq!(report.sources_pushed, 0);
    assert_eq!(report.state.queue_len, 1);
    assert_eq!(
        read_json(&scope.account_file("sync_queue.json")),
        json!([{ "appid": "steam:1", "apiname": "ACH01", "unlocked_at": 1, "achieved": true }])
    );
    let paths: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.url.path().to_string())
        .collect();
    assert!(!paths.iter().any(|path| path.contains("push_library")));
    assert!(!paths.iter().any(|path| path.contains("sync_achievements")));
    assert!(!paths.iter().any(|path| path.contains("push_sources")));
    assert!(!paths
        .iter()
        .any(|path| path.contains("push_steam_playtime")));
    assert_eq!(events.lock().unwrap().len(), 1);
    assert!(matches!(
        events.lock().unwrap().first(),
        Some(SyncEvent::SyncState(_))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn source_push_can_be_enabled_without_unlocking_other_domains() {
    let server = MockServer::start().await;
    let dir = temp_dir("source-push-flag");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("sources.json"),
        json!([{
            "id": "a",
            "url": "https://example/a.json",
            "name": "A",
        }]),
    );

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_achievements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_sources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/push_sources"))
        .and(body_json(json!({
            "p_sources": [{ "source_id": "a", "url": "https://example/a.json", "name": "A" }]
        })))
        .respond_with(null_response())
        .expect(1)
        .mount(&server)
        .await;

    let engine = SyncEngine::new(client, scope.clone());
    assert!(engine.pull_only());
    engine.set_push_sources(true);
    let report = engine.sync_now().await.unwrap();
    assert_eq!(report.sources_pushed, 1);
    assert_eq!(report.library_pushed, 0);
    assert_eq!(report.achievements_pushed, 0);

    let paths: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.url.path().to_string())
        .collect();
    assert!(paths.iter().any(|path| path.contains("push_sources")));
    assert!(!paths.iter().any(|path| path.contains("push_library")));
    assert!(!paths.iter().any(|path| path.contains("sync_achievements")));
    assert!(!paths
        .iter()
        .any(|path| path.contains("push_steam_playtime")));

    let state = read_json(&scope.account_file("sync_state.json"));
    assert_eq!(state["srcPush"]["a"]["url"], "https://example/a.json");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn reconcile_pull_only_applies_library_and_emits_events() {
    let server = MockServer::start().await;
    let dir = temp_dir("reconcile");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "appid": "custom:remote", "title": "Remoto", "platform": "windows" }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_achievements"))
        .and(body_json(json!({ "p_since": null })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_sources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let events: Arc<Mutex<Vec<SyncEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let engine = SyncEngine::new(client, scope.clone())
        .with_event_listener(move |event| sink.lock().unwrap().push(event.clone()));

    let report = engine.reconcile().await.unwrap();
    assert!(report.library_changed);
    assert_eq!(report.library_pulled, 1);
    assert_eq!(report.achievements_pulled, 0);
    assert_eq!(report.sources_pulled, 0);
    assert_eq!(events.lock().unwrap().len(), 2);
    assert_eq!(events.lock().unwrap()[0], SyncEvent::LibraryChanged);
    assert!(matches!(
        events.lock().unwrap()[1],
        SyncEvent::SyncState(SyncState { queue_len: 0, .. })
    ));

    let custom = read_json(&scope.account_file("custom_games.json"));
    assert_eq!(custom[0]["id"], "custom:remote");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn network_failure_preserves_local_state_and_reports_rede_indisponivel() {
    // Socket que aceita conexões e nunca responde força timeout de transporte,
    // sem qualquer acesso à rede externa.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let config = ApiConfig::resolve(Some(&format!("http://{address}")), None, None, None);
    let client = ApiClient::with_timeout(config, Duration::from_millis(250)).unwrap();
    install_session(&client).await;

    let dir = temp_dir("network");
    let scope = scope_for(&dir);
    write_json(
        &scope.account_file("custom_games.json"),
        json!([{ "id": "custom:1", "title": "Local", "launcher": "custom" }]),
    );
    write_json(
        &scope.account_file("sync_queue.json"),
        json!([{ "appid": "steam:1", "apiname": "ACH01", "unlocked_at": 1, "achieved": true }]),
    );
    let before_custom = std::fs::read_to_string(scope.account_file("custom_games.json")).unwrap();
    let before_queue = std::fs::read_to_string(scope.account_file("sync_queue.json")).unwrap();

    let engine = SyncEngine::new(client, scope.clone());
    let error = engine.sync_now().await.unwrap_err();
    assert_eq!(error.code(), "rede_indisponivel");
    assert!(error.is_retryable());
    assert_eq!(
        std::fs::read_to_string(scope.account_file("custom_games.json")).unwrap(),
        before_custom
    );
    assert_eq!(
        std::fs::read_to_string(scope.account_file("sync_queue.json")).unwrap(),
        before_queue
    );
    assert_eq!(
        engine.state().last_error.as_deref(),
        Some("rede_indisponivel")
    );
    let _ = std::fs::remove_dir_all(&dir);
    drop(listener);
}

#[tokio::test]
async fn auth_401_propagates_typed_error_and_stops_before_other_domains() {
    let server = MockServer::start().await;
    let dir = temp_dir("auth");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;
    write_json(
        &scope.account_file("custom_games.json"),
        json!([{ "id": "custom:1", "title": "Local", "launcher": "custom" }]),
    );
    let before = std::fs::read_to_string(scope.account_file("custom_games.json")).unwrap();

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "error": "jwt expired" })))
        .expect(1)
        .mount(&server)
        .await;

    let engine = SyncEngine::new(client, scope.clone());
    let error = engine.sync_now().await.unwrap_err();
    assert_eq!(error, SyncError::Unauthorized);
    assert!(!error.is_retryable());
    assert_eq!(engine.state().last_error.as_deref(), Some("token_invalido"));
    assert_eq!(
        std::fs::read_to_string(scope.account_file("custom_games.json")).unwrap(),
        before
    );
    // A falha de auth é fatal: conquistas/sources nem são tentados.
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn backend_failure_does_not_clear_achievement_queue() {
    let server = MockServer::start().await;
    let dir = temp_dir("backend-fail");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;
    let queue = json!([{
        "appid": "steam:1",
        "apiname": "ACH01",
        "unlocked_at": 1_700_000_000,
        "achieved": true
    }]);
    write_json(&scope.account_file("sync_queue.json"), queue.clone());

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/sync_achievements"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({ "error": "boom" })))
        .expect(1)
        .mount(&server)
        .await;

    let engine = SyncEngine::with_pull_only(client, scope.clone(), false);
    let error = engine.push_achievements().await.unwrap_err();
    assert_eq!(
        error,
        SyncError::Backend {
            message: "boom".into(),
            retryable: true
        }
    );
    assert_eq!(read_json(&scope.account_file("sync_queue.json")), queue);
    assert_eq!(engine.state().last_error.as_deref(), Some("boom"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn sync_all_pull_only_uses_full_pull_and_never_pushes_local_catchup() {
    let server = MockServer::start().await;
    let dir = temp_dir("sync-all");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("achievements.json"),
        json!({
            "steam:1": {
                "at": 1,
                "items": [{ "apiname": "ACH01", "title": "Um", "achieved": true, "unlock": 100 }]
            }
        }),
    );

    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_achievements"))
        .and(body_json(json!({ "p_since": null })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "appid": "steam:2", "apiname": "ACH02", "unlocked_at": 50, "title": "Dois" }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_sources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let engine = SyncEngine::new(client, scope.clone());
    let report = engine.sync_all().await.unwrap();
    assert!(report.full);
    assert_eq!(report.achievements_pushed, 0);
    assert_eq!(report.achievements_pulled, 1);
    let paths: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| request.url.path().to_string())
        .collect();
    assert!(!paths.iter().any(|path| path.contains("sync_achievements")));
    let store = read_json(&scope.account_file("achievements.json"));
    assert_eq!(store["steam:2"]["items"][0]["unlock"], 50);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unchanged_library_push_is_a_noop_without_rpc() {
    let server = MockServer::start().await;
    let dir = temp_dir("noop");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;

    write_json(
        &scope.account_file("custom_games.json"),
        json!([{ "id": "custom:1", "title": "Jogo", "launcher": "custom" }]),
    );
    write_json(
        &scope.account_file("sync_state.json"),
        json!({
            "libPush": { "custom:1": { "title": "Jogo", "platform": "windows" } },
            "playtimePush": {}
        }),
    );
    arcadia_store::owned::write_owned(&scope, &[]).unwrap();

    let outcome = library::push(&client, &scope).await.unwrap();
    assert_eq!(outcome, library::LibraryPush::default());
    assert!(server.received_requests().await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cloned_engine_serializes_pull_library() {
    // Sanidade de clone: a trava é compartilhada e não corrompe arquivos.
    let server = MockServer::start().await;
    let dir = temp_dir("clone");
    let scope = scope_for(&dir);
    let client = client_for(&server);
    install_session(&client).await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_library"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/pull_steam_playtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let engine = SyncEngine::new(client, scope.clone());
    let clone = engine.clone();
    let (left, right) = tokio::join!(engine.pull_library(), clone.pull_library());
    assert!(left.is_ok());
    assert!(right.is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}
