//! Contratos do cliente de catálogo: wiremock local para caminho/headers e
//! fetcher roteirizado para sequências (304, 401, timeout, dedupe).

mod support;

use std::sync::Arc;
use std::time::Duration;

use arcadia_catalog::catalog::{
    mirror_file_name, CatalogClient, CatalogOptions, FsMirrorStore, MemoryMirrorStore, MirrorStore,
    StaticAuth, FULL_TIMEOUT, MIRROR_TIMEOUT,
};
use arcadia_catalog::http::{Fetcher, Response};
use serde_json::json;
use support::{json_response, ScriptedAuth, ScriptedFetcher};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn memory_client(fetcher: Arc<dyn Fetcher>, base: &str) -> (CatalogClient, Arc<MemoryMirrorStore>) {
    let store = Arc::new(MemoryMirrorStore::new());
    let auth = Arc::new(StaticAuth::with_anon_key(
        "anon-key",
        Some("token-1".into()),
    ));
    (
        CatalogClient::new(fetcher, store.clone(), auth, base),
        store,
    )
}

#[tokio::test]
async fn catalog_get_hits_exact_backend_path_and_headers_and_mirrors_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/catalog/v1/popular"))
        .and(query_param("limite", "1000"))
        .and(header("accept", "application/json"))
        .and(header("accept-encoding", "gzip"))
        .and(header("apikey", "anon-key"))
        .and(header("authorization", "Bearer token-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_json(json!({ "ok": true, "itens": [{ "appid": "1" }] })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let store = Arc::new(MemoryMirrorStore::new());
    let auth = Arc::new(StaticAuth::with_anon_key(
        "anon-key",
        Some("token-1".into()),
    ));
    let client = CatalogClient::from_base_url(&server.uri(), store.clone(), auth).unwrap();

    let response = client
        .get_with("/catalog/v1/popular?limite=1000", CatalogOptions::default())
        .await;

    assert_eq!(response.error, None);
    assert_eq!(response.fallback, Some(false));
    assert_eq!(response.data.as_ref().unwrap()["itens"][0]["appid"], "1");
    let name = mirror_file_name("/catalog/v1/popular?limite=1000");
    assert_eq!(
        response.data.as_ref().unwrap(),
        &store.read_json(&name).unwrap()
    );
    assert_eq!(store.read_etag(&name).as_deref(), Some("\"v1\""));
}

#[tokio::test]
async fn mirror_304_returns_local_data_without_redownloading() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::json(200, &json!({ "ok": true, "itens": [1] })).header("etag", "\"v1\""),
        Response::new(304, ""),
    ]);
    let (client, store) = memory_client(fetcher.clone(), "https://arcadia.test");

    let first = client.get("/catalog/v1/popular").await;
    assert_eq!(first.fallback, Some(false));
    let second = client.get("/catalog/v1/popular").await;

    assert_eq!(second.data, first.data);
    assert_eq!(second.error, None);
    assert_eq!(second.fallback, Some(true));
    assert_eq!(second.not_modified, Some(true));

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        support::header(&requests[1], "if-none-match"),
        Some("\"v1\"")
    );
    assert!(store.read_json("popular.json").is_some());
}

#[tokio::test]
async fn timeout_is_30s_without_mirror_5s_with_mirror_and_override_wins() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::json(200, &json!({ "ok": true })),
        Response::new(304, ""),
        Response::new(304, ""),
    ]);
    let (client, _store) = memory_client(fetcher.clone(), "https://arcadia.test");

    let _ = client.get("/catalog/v1/popular").await;
    let _ = client.get("/catalog/v1/popular").await;
    let _ = client
        .get_with(
            "/catalog/v1/popular",
            CatalogOptions::default().with_timeout_ms(7_000),
        )
        .await;

    let requests = fetcher.requests();
    assert_eq!(requests[0].timeout, FULL_TIMEOUT);
    assert_eq!(requests[1].timeout, MIRROR_TIMEOUT);
    assert_eq!(requests[2].timeout, Duration::from_secs(7));
}

#[tokio::test]
async fn server_error_falls_back_to_mirror_and_without_mirror_returns_http_error() {
    let store = Arc::new(MemoryMirrorStore::new());
    store.write_json("popular.json", &json!({ "ok": true, "cached": true }));
    let fetcher = ScriptedFetcher::new(vec![Response::text(500, "boom")]);
    let auth = Arc::new(StaticAuth::default());
    let client = CatalogClient::new(fetcher, store.clone(), auth, "https://arcadia.test");

    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.error, None);
    assert_eq!(response.fallback, Some(true));
    assert_eq!(response.data.unwrap()["cached"], true);

    let fetcher = ScriptedFetcher::new(vec![Response::text(500, "boom")]);
    let (client, _store) = memory_client(fetcher, "https://arcadia.test");
    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.data, None);
    assert_eq!(response.error.unwrap().message, "HTTP 500");
    assert_eq!(response.fallback, None);
}

#[tokio::test]
async fn no_mirror_never_reads_or_writes_etag_and_has_no_fallback() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::json(
            200,
            &json!({ "ok": true, "offer": { "uris": ["magnet:?x"] } }),
        )
        .header("etag", "\"secret\""),
        Response::text(500, "offline"),
    ]);
    let (client, store) = memory_client(fetcher.clone(), "https://arcadia.test");
    let options = CatalogOptions::default().with_no_mirror(true);

    let online = client
        .get_with("/catalog/v1/retro/offers/private", options)
        .await;
    assert_eq!(online.error, None);
    assert_eq!(online.data.unwrap()["offer"]["uris"][0], "magnet:?x");
    assert_eq!(store.len(), 0);

    let offline = client
        .get_with("/catalog/v1/retro/offers/private", options)
        .await;
    assert_eq!(offline.data, None);
    assert_eq!(offline.error.unwrap().message, "HTTP 500");
    assert_eq!(offline.fallback, None);

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 2);
    assert!(support::header(&requests[0], "if-none-match").is_none());
    assert!(support::header(&requests[1], "if-none-match").is_none());
}

#[tokio::test]
async fn invalid_path_is_rejected_before_any_request() {
    let fetcher = ScriptedFetcher::new(vec![]);
    let (client, _store) = memory_client(fetcher.clone(), "https://arcadia.test");

    for path in ["", "/popular", "/catalog/v2/popular", "catalog/v1/popular"] {
        let response = client.get(path).await;
        assert_eq!(response.data, None);
        assert_eq!(
            response.error.unwrap().message,
            "caminho de catalogo invalido"
        );
    }
    assert_eq!(fetcher.request_count(), 0);
}

#[tokio::test]
async fn expired_token_refreshes_once_and_retries_with_the_new_header() {
    let auth = ScriptedAuth::new();
    let fetcher = ScriptedFetcher::new(vec![
        Response::text(401, "expired"),
        Response::json(200, &json!({ "ok": true, "itens": [{ "appid": "10" }] })),
    ]);
    let store = Arc::new(MemoryMirrorStore::new());
    let client = CatalogClient::new(fetcher.clone(), store, auth.clone(), "https://arcadia.test");

    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.error, None);
    assert_eq!(response.data.unwrap()["itens"][0]["appid"], "10");
    assert_eq!(auth.refresh_count(), 1);

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        support::header(&requests[0], "authorization"),
        Some("Bearer token-1")
    );
    assert_eq!(
        support::header(&requests[1], "authorization"),
        Some("Bearer token-2")
    );
}

#[tokio::test]
async fn persistent_401_falls_back_to_mirror_then_reports_http_401() {
    let store = Arc::new(MemoryMirrorStore::new());
    store.write_json("popular.json", &json!({ "ok": true, "cached": true }));
    let auth = ScriptedAuth::new();
    let fetcher = ScriptedFetcher::new(vec![
        Response::text(401, "nope"),
        Response::text(401, "nope"),
    ]);
    let client = CatalogClient::new(fetcher.clone(), store, auth, "https://arcadia.test");

    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.fallback, Some(true));
    assert_eq!(response.data.unwrap()["cached"], true);
    assert_eq!(fetcher.request_count(), 2);

    let (client, _store) = memory_client(
        ScriptedFetcher::new(vec![Response::text(401, "nope"); 2]),
        "https://arcadia.test",
    );
    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.error.unwrap().message, "HTTP 401");
    assert_eq!(response.fallback, None);
}

#[tokio::test]
async fn malformed_json_falls_back_to_mirror_or_reports_decode_error() {
    let store = Arc::new(MemoryMirrorStore::new());
    store.write_json("popular.json", &json!({ "ok": true, "cached": true }));
    let client = CatalogClient::new(
        ScriptedFetcher::new(vec![Response::text(200, "<html>not json</html>")]),
        store,
        Arc::new(StaticAuth::default()),
        "https://arcadia.test",
    );
    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.fallback, Some(true));
    assert_eq!(response.data.unwrap()["cached"], true);

    let (client, _store) = memory_client(
        ScriptedFetcher::new(vec![Response::text(200, "<html>not json</html>")]),
        "https://arcadia.test",
    );
    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.data, None);
    assert!(response
        .error
        .unwrap()
        .message
        .contains("resposta JSON inválida"));
}

#[tokio::test]
async fn network_failure_without_mirror_returns_transport_message() {
    let fetcher = ScriptedFetcher::with_results(vec![Err(arcadia_catalog::http::FetchError::new(
        "ECONNREFUSED",
    ))]);
    let (client, _store) = memory_client(fetcher, "https://arcadia.test");
    let response = client.get("/catalog/v1/popular").await;
    assert_eq!(response.data, None);
    assert_eq!(response.error.unwrap().message, "ECONNREFUSED");
}

#[tokio::test]
async fn concurrent_identical_gets_share_one_in_flight_request() {
    let fetcher = ScriptedFetcher::with_delay(
        vec![Ok(json_response(json!({ "ok": true, "itens": [1] })))],
        Duration::from_millis(40),
    );
    let (client, _store) = memory_client(fetcher.clone(), "https://arcadia.test");
    let client = Arc::new(client);

    let (first, second) = tokio::join!(
        client.get("/catalog/v1/popular"),
        client.get("/catalog/v1/popular")
    );
    assert_eq!(first, second);
    assert_eq!(fetcher.request_count(), 1);
}

#[tokio::test]
async fn fs_mirror_store_writes_json_and_etag_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsMirrorStore::new(dir.path());
    assert!(!store.exists("popular.json"));
    store.write_json("popular.json", &json!({ "ok": true }));
    store.write_etag("popular.json", "\"abc\"");
    assert!(store.path("popular.json").exists());
    assert_eq!(store.read_json("popular.json").unwrap()["ok"], true);
    assert_eq!(store.read_etag("popular.json").as_deref(), Some("\"abc\""));
    assert!(!store.path("popular.json.tmp").exists());
    assert!(store.dir().ends_with("catalog_espelho"));
}
