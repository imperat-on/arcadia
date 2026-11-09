//! Contratos dos hosters/debrids: endpoints, corpos, seleção de link, erros e
//! ordem de fallback. Nenhuma chamada sai para a internet.

mod support;

use std::sync::Arc;
use std::time::Duration;

use arcadia_catalog::hosters::{
    AbortSignal, AtomicAbort, DebridClient, DebridEndpoints, DebridProvider, DebridTokens,
    NeverAbort, NoopSleeper, PollWindow,
};
use arcadia_catalog::http::{Fetcher, HttpClientFetcher, Response};
use serde_json::json;
use support::{body_text, header, json_response, ScriptedFetcher};
use wiremock::matchers::{body_string_contains, header as wm_header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn tokens(provider: DebridProvider, token: &str) -> DebridTokens {
    let mut tokens = DebridTokens::new();
    tokens.set(provider, token);
    tokens
}

fn local_endpoints(base: &str) -> DebridEndpoints {
    DebridEndpoints {
        real_debrid: base.to_string(),
        tor_box: base.to_string(),
        all_debrid: base.to_string(),
        premiumize: base.to_string(),
        gofile_cdn: base.to_string(),
        pixeldrain_bypass: base.to_string(),
        pixeldrain_api: format!("{base}/api/file"),
        rootz_web: base.to_string(),
    }
}

fn test_client(fetcher: Arc<ScriptedFetcher>, tokens: DebridTokens) -> DebridClient {
    DebridClient::with_tokens(fetcher, tokens)
        .with_endpoints(local_endpoints("https://debrid.test"))
        .with_sleeper(Arc::new(NoopSleeper))
        .with_poll_windows(
            PollWindow::new(Duration::from_millis(1), Duration::from_millis(5)),
            PollWindow::new(Duration::from_millis(1), Duration::from_millis(5)),
        )
}

#[tokio::test]
async fn real_debrid_unrestrict_hits_exact_endpoint_with_bearer_and_form_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/1.0/unrestrict/link"))
        .and(wm_header("authorization", "Bearer rd-token"))
        .and(wm_header(
            "content-type",
            "application/x-www-form-urlencoded",
        ))
        .and(body_string_contains(
            "link=https%3A%2F%2Fexample.com%2Ffile%3Fx%3D1%26y%3D2",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "download": "https://cdn.real-debrid.test/game"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let fetcher: Arc<dyn Fetcher> =
        Arc::new(HttpClientFetcher::from_base_url(&server.uri()).unwrap());
    let client = DebridClient::with_tokens(fetcher, tokens(DebridProvider::RealDebrid, "rd-token"))
        .with_endpoints(local_endpoints(&server.uri()));
    let link = client
        .unrestrict(
            DebridProvider::RealDebrid,
            "https://example.com/file?x=1&y=2",
        )
        .await
        .unwrap();
    assert_eq!(link.url, "https://cdn.real-debrid.test/game");
}

#[tokio::test]
async fn all_debrid_unlock_uses_agent_and_apikey_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v4/link/unlock"))
        .and(query_param("agent", "arcadia"))
        .and(query_param("apikey", "ad-token"))
        .and(body_string_contains(
            "link=https%3A%2F%2Fexample.com%2Fpage",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": { "link": "https://cdn.alldebrid.test/file" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let fetcher: Arc<dyn Fetcher> =
        Arc::new(HttpClientFetcher::from_base_url(&server.uri()).unwrap());
    let client = DebridClient::with_tokens(fetcher, tokens(DebridProvider::AllDebrid, "ad-token"))
        .with_endpoints(local_endpoints(&server.uri()));
    let link = client
        .unrestrict(DebridProvider::AllDebrid, "https://example.com/page")
        .await
        .unwrap();
    assert_eq!(link.url, "https://cdn.alldebrid.test/file");
}

#[tokio::test]
async fn premiumize_directdl_uses_apikey_query_and_src_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/transfer/directdl"))
        .and(query_param("apikey", "pz-token"))
        .and(body_string_contains("src=https%3A%2F%2Fexample.com%2Fpage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "content": [{ "link": "https://cdn.premiumize.test/file" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let fetcher: Arc<dyn Fetcher> =
        Arc::new(HttpClientFetcher::from_base_url(&server.uri()).unwrap());
    let client = DebridClient::with_tokens(fetcher, tokens(DebridProvider::Premiumize, "pz-token"))
        .with_endpoints(local_endpoints(&server.uri()));
    let link = client
        .unrestrict(DebridProvider::Premiumize, "https://example.com/page")
        .await
        .unwrap();
    assert_eq!(link.url, "https://cdn.premiumize.test/file");
}

#[tokio::test]
async fn real_debrid_errors_match_the_frozen_messages() {
    let fetcher = ScriptedFetcher::new(vec![Response::text(503, "down")]);
    let client = test_client(fetcher, tokens(DebridProvider::RealDebrid, "rd-token"));
    let error = client
        .unrestrict(DebridProvider::RealDebrid, "https://example.com/file")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Real-Debrid: HTTP 503 (token inválido ou conta expirada?)"
    );

    let fetcher = ScriptedFetcher::new(vec![Response::json(200, &json!({ "id": "x" }))]);
    let client = test_client(fetcher, tokens(DebridProvider::RealDebrid, "rd-token"));
    let error = client
        .unrestrict(DebridProvider::RealDebrid, "https://example.com/file")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Real-Debrid não liberou este link (hoster fora do catálogo deles)"
    );
}

#[tokio::test]
async fn all_debrid_and_premiumize_error_messages_come_from_json() {
    let fetcher = ScriptedFetcher::new(vec![Response::json(
        200,
        &json!({ "status": "error", "error": { "message": "hoster fora" } }),
    )]);
    let client = test_client(fetcher, tokens(DebridProvider::AllDebrid, "ad-token"));
    let error = client
        .unrestrict(DebridProvider::AllDebrid, "https://example.com/f")
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "AllDebrid: hoster fora");

    let fetcher = ScriptedFetcher::new(vec![Response::json(
        200,
        &json!({ "status": "error", "message": "limite diário" }),
    )]);
    let client = test_client(fetcher, tokens(DebridProvider::Premiumize, "pz-token"));
    let error = client
        .unrestrict(DebridProvider::Premiumize, "https://example.com/f")
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Premiumize: limite diário");
}

#[tokio::test]
async fn torbox_webdl_flow_polls_mylist_and_picks_the_largest_file() {
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({ "success": true, "data": { "webdownload_id": 7 } })),
        json_response(json!({
            "success": true,
            "data": [
                { "id": 6, "download_state": "queued", "files": [] },
                {
                    "id": 7,
                    "download_finished": true,
                    "files": [
                        { "id": 11, "size": 1000 },
                        { "id": 12, "size": 9999 },
                        { "id": 13, "size": 500 }
                    ]
                }
            ]
        })),
        json_response(json!({ "success": true, "data": "https://cdn.torbox.test/7" })),
    ]);
    let client = test_client(fetcher.clone(), tokens(DebridProvider::TorBox, "tb-token"));
    let link = client
        .unrestrict(DebridProvider::TorBox, "https://host.example/file")
        .await
        .unwrap();
    assert_eq!(link.url, "https://cdn.torbox.test/7");

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, arcadia_catalog::http::Method::Post);
    assert!(requests[0].url.ends_with("/v1/api/webdl/createwebdownload"));
    assert_eq!(
        header(&requests[0], "authorization"),
        Some("Bearer tb-token")
    );
    assert_eq!(header(&requests[0], "user-agent"), Some("arcadia"));
    assert!(header(&requests[0], "content-type")
        .unwrap()
        .starts_with("multipart/form-data; boundary="));
    let create_body = body_text(&requests[0]);
    assert!(create_body.contains("name=\"link\""));
    assert!(create_body.contains("https://host.example/file"));

    assert_eq!(
        requests[1].url,
        "https://debrid.test/v1/api/webdl/mylist?id=7"
    );
    assert_eq!(
        requests[2].url,
        "https://debrid.test/v1/api/webdl/requestdl?token=tb-token&web_id=7&file_id=12"
    );
}

#[tokio::test]
async fn torbox_webdl_not_cached_yet_reports_the_timeout_message() {
    let mut responses = vec![json_response(
        json!({ "success": true, "data": { "webdownload_id": 8 } }),
    )];
    for _ in 0..4 {
        responses.push(json_response(json!({
            "success": true,
            "data": { "id": 8, "download_state": "downloading", "files": [] }
        })));
    }
    let fetcher = ScriptedFetcher::new(responses);
    let client = test_client(fetcher, tokens(DebridProvider::TorBox, "tb-token"));
    let error = client
        .unrestrict(DebridProvider::TorBox, "https://host.example/file")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "TorBox: link ainda não cacheado (tente de novo em alguns minutos)"
    );
}

#[tokio::test]
async fn real_debrid_magnet_selects_largest_selected_and_unrestricts_it() {
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({ "id": "abc" })),
        Response::new(204, ""),
        json_response(json!({
            "status": "downloaded",
            "files": [
                { "selected": true, "bytes": 10 },
                { "selected": false, "bytes": 9999 },
                { "selected": true, "bytes": 500 }
            ],
            "links": ["small-link", "big-link"]
        })),
        json_response(json!({ "download": "https://direct.rd/game" })),
    ]);
    let client = test_client(
        fetcher.clone(),
        tokens(DebridProvider::RealDebrid, "rd-token"),
    );
    let link = client
        .resolve_magnet("magnet:?xt=urn:btih:abc", &NeverAbort)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://direct.rd/game");

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].url.ends_with("/rest/1.0/torrents/addMagnet"));
    assert!(body_text(&requests[0]).starts_with("magnet=magnet%3A%3Fxt%3Durn%3Abtih%3Aabc"));
    assert!(requests[1]
        .url
        .ends_with("/rest/1.0/torrents/selectFiles/abc"));
    assert_eq!(body_text(&requests[1]), "files=all");
    assert!(requests[2].url.ends_with("/rest/1.0/torrents/info/abc"));
    assert!(requests[3].url.ends_with("/rest/1.0/unrestrict/link"));
    assert!(body_text(&requests[3]).contains("link=big-link"));
}

#[tokio::test]
async fn real_debrid_magnet_without_selected_files_errors() {
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({ "id": "abc" })),
        Response::new(204, ""),
        json_response(json!({
            "status": "downloaded",
            "files": [{ "selected": false, "bytes": 9999 }],
            "links": []
        })),
    ]);
    let client = test_client(fetcher, tokens(DebridProvider::RealDebrid, "rd-token"));
    let error = client
        .resolve_magnet("magnet:?xt=urn:btih:abc", &NeverAbort)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Real-Debrid: nenhum arquivo no torrent");
}

#[tokio::test]
async fn torbox_magnet_uses_zip_link_and_cached_state() {
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({ "success": true, "data": { "torrent_id": 9 } })),
        json_response(json!({
            "success": true,
            "data": { "id": 9, "download_state": "cached" }
        })),
        json_response(json!({ "success": true, "data": "https://cdn.torbox.test/9.zip" })),
    ]);
    let client = test_client(fetcher.clone(), tokens(DebridProvider::TorBox, "tb-token"));
    let link = client
        .resolve_magnet("magnet:?xt=urn:btih:def", &NeverAbort)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://cdn.torbox.test/9.zip");

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].url.ends_with("/v1/api/torrents/createtorrent"));
    assert!(body_text(&requests[0]).contains("name=\"magnet\""));
    assert_eq!(
        requests[2].url,
        "https://debrid.test/v1/api/torrents/requestdl?token=tb-token&torrent_id=9&zip_link=true"
    );
}

#[tokio::test]
async fn alldebrid_magnet_walks_file_tree_and_unlocks_the_largest() {
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({
            "status": "success",
            "data": { "magnets": { "id": 42 } }
        })),
        json_response(json!({
            "status": "success",
            "data": { "magnets": [{ "status": "Ready" }] }
        })),
        json_response(json!({
            "status": "success",
            "data": { "magnets": [{ "files": [
                { "n": "small.txt", "s": 5, "l": "https://ad/small" },
                { "n": "folder", "e": [
                    { "n": "game.iso", "s": 100, "l": "https://ad/big" }
                ]}
            ]}]}
        })),
        json_response(json!({
            "status": "success",
            "data": { "link": "https://direct.ad/game" }
        })),
    ]);
    let client = test_client(
        fetcher.clone(),
        tokens(DebridProvider::AllDebrid, "ad-token"),
    );
    let link = client
        .resolve_magnet("magnet:?xt=urn:btih:ghi", &NeverAbort)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://direct.ad/game");

    let requests = fetcher.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0]
        .url
        .contains("/v4.1/magnet/upload?agent=arcadia&apikey=ad-token"));
    assert!(body_text(&requests[0]).contains("magnets%5B%5D=magnet%3A%3Fxt"));
    assert!(requests[1].url.contains("/v4.1/magnet/status"));
    assert!(requests[2].url.contains("/v4.1/magnet/files"));
    assert!(requests[3].url.contains("/v4/link/unlock"));
    assert!(body_text(&requests[3]).contains("link=https%3A%2F%2Fad%2Fbig"));
}

#[tokio::test]
async fn premiumize_magnet_uses_directdl() {
    let fetcher = ScriptedFetcher::new(vec![json_response(json!({
        "status": "success",
        "content": [{ "link": "https://direct.pz/game" }]
    }))]);
    let client = test_client(
        fetcher.clone(),
        tokens(DebridProvider::Premiumize, "pz-token"),
    );
    let link = client
        .resolve_magnet("magnet:?xt=urn:btih:jkl", &NeverAbort)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://direct.pz/game");
    let requests = fetcher.requests();
    assert!(requests[0]
        .url
        .contains("/api/transfer/directdl?apikey=pz-token"));
    assert!(body_text(&requests[0]).contains("src=magnet%3A%3Fxt"));
}

#[tokio::test]
async fn free_gofile_resolver_runs_before_configured_debrids() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::new(200, "").header("content-type", "application/octet-stream")
    ]);
    let client = test_client(
        fetcher.clone(),
        tokens(DebridProvider::RealDebrid, "rd-token"),
    );
    let link = client
        .resolve_hoster("https://gofile.io/d/abc")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://debrid.test/abc");
    let requests = fetcher.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(header(&requests[0], "range"), Some("bytes=0-0"));
    assert_eq!(
        header(&requests[0], "user-agent"),
        Some(arcadia_catalog::hosters::BROWSER_UA)
    );
}

#[tokio::test]
async fn pixeldrain_falls_back_to_official_api_when_bypass_is_html() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::new(200, "<html>").header("content-type", "text/html; charset=utf-8")
    ]);
    let client = test_client(fetcher, DebridTokens::new());
    let link = client
        .resolve_hoster("https://pixeldrain.com/u/xyz")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://debrid.test/api/file/xyz?download");
}

#[tokio::test]
async fn rootz_reads_page_token_and_returns_proxy_with_referer() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::text(200, r#"<html>{"pageToken":"tok-1"}</html>"#),
        json_response(json!({
            "success": true,
            "data": { "fileId": "f1", "status": "active", "downloadAllowed": true }
        })),
    ]);
    let client = test_client(fetcher.clone(), DebridTokens::new());
    let link = client
        .resolve_hoster("https://rootz.so/d/r1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://debrid.test/api/files/proxy-download/f1");
    assert_eq!(
        link.headers,
        vec![(
            "Referer".to_string(),
            "https://debrid.test/d/r1".to_string()
        )]
    );

    let requests = fetcher.requests();
    assert_eq!(header(&requests[1], "x-page-token"), Some("tok-1"));
    assert_eq!(
        header(&requests[1], "referer"),
        Some("https://debrid.test/d/r1")
    );
}

#[tokio::test]
async fn free_resolver_failure_falls_back_to_debrid() {
    let fetcher = ScriptedFetcher::new(vec![
        Response::new(200, "<html>").header("content-type", "text/html"),
        json_response(json!({ "download": "https://direct.rd/fallback" })),
    ]);
    let client = test_client(fetcher, tokens(DebridProvider::RealDebrid, "rd-token"));
    let link = client
        .resolve_hoster("https://gofile.io/d/abc")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.url, "https://direct.rd/fallback");
}

#[tokio::test]
async fn debrid_errors_are_joined_in_provider_order() {
    let mut tokens = DebridTokens::new();
    tokens.set(DebridProvider::RealDebrid, "rd");
    tokens.set(DebridProvider::AllDebrid, "ad");
    tokens.set(DebridProvider::Premiumize, "pz");
    let fetcher = ScriptedFetcher::new(vec![
        Response::text(500, "rd down"),
        json_response(json!({ "status": "error", "error": { "message": "ad falhou" } })),
        json_response(json!({ "status": "error", "message": "pz falhou" })),
    ]);
    let client = test_client(fetcher, tokens);
    let error = client
        .resolve_hoster("https://example.com/page")
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Real-Debrid: HTTP 500 (token inválido ou conta expirada?) | AllDebrid: ad falhou | Premiumize: pz falhou"
    );
}

#[tokio::test]
async fn no_tokens_returns_none_and_direct_unrestrict_reports_not_configured() {
    let fetcher = ScriptedFetcher::new(vec![]);
    let client = test_client(fetcher.clone(), DebridTokens::new());
    assert_eq!(
        client
            .resolve_hoster("https://example.com/page")
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        client
            .resolve_magnet("magnet:?xt=urn:btih:abc", &NeverAbort)
            .await
            .unwrap(),
        None
    );
    assert_eq!(fetcher.request_count(), 0);
    assert_eq!(
        client
            .unrestrict(DebridProvider::TorBox, "https://example.com/f")
            .await
            .unwrap_err()
            .to_string(),
        "TorBox: token nao configurado"
    );
}

#[tokio::test]
async fn aborted_magnet_returns_none_without_polling() {
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({ "id": "abc" })),
        Response::new(204, ""),
    ]);
    let client = test_client(
        fetcher.clone(),
        tokens(DebridProvider::RealDebrid, "rd-token"),
    );
    let abort = AtomicAbort::new();
    abort.abort();
    assert!(abort.is_aborted());
    let result = client
        .resolve_magnet("magnet:?xt=urn:btih:abc", &abort)
        .await
        .unwrap();
    assert_eq!(result, None);
    // addMagnet + selectFiles; o poll nem começa.
    assert_eq!(fetcher.request_count(), 2);
}

#[tokio::test]
async fn transport_errors_never_leak_the_token_bearing_url() {
    let fetcher = ScriptedFetcher::with_results(vec![Err(arcadia_catalog::http::FetchError::new(
        "error sending request for url https://api.alldebrid.com/v4/link/unlock?apikey=SECRET",
    ))]);
    let client = test_client(fetcher, tokens(DebridProvider::AllDebrid, "SECRET"));
    let error = client
        .unrestrict(DebridProvider::AllDebrid, "https://example.com/f")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "AllDebrid: falha de rede");
    assert!(!error.contains("SECRET"));
}
