//! Contratos HTTP contra um servidor local (wiremock).
//!
//! Usa o [`HttpClientFetcher`] de produção sobre o `arcadia-api`, então testa
//! de ponta a ponta: URL, query, cabeçalhos, corpo e parsing. Nenhuma chamada
//! sai para a internet.

use std::sync::Arc;
use std::time::Duration;

use arcadia_media::art::download_to;
use arcadia_media::providers::psn::PsnClient;
use arcadia_media::providers::steamgriddb::{SgdbArtOptions, SteamGridDb};
use arcadia_media::{
    ArtKind, Fetcher, HltbClient, HttpClientFetcher, IgdbClient, IgdbSearch, SteamClient,
    XboxClient,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fetcher(server: &MockServer) -> Arc<HttpClientFetcher> {
    Arc::new(HttpClientFetcher::from_base_url(&server.uri()).unwrap())
}

#[tokio::test]
async fn sgdb_search_and_art_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/search/autocomplete/portal2"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": [{"id": 10, "name": "Portal 2", "release_date": "2011-04-19"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/grids/game/10"))
        .and(query_param("types", "static,animated"))
        .and(query_param("dimensions", "600x900,660x930,342x482"))
        .and(query_param("nsfw", "false"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": [{
                "url": "https://cdn.example/grid/abc.png",
                "thumb": "https://cdn.example/thumb/abc.jpg",
                "width": 600,
                "height": 900,
                "mime": "image/png"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = SteamGridDb::new(fetcher(&server), "test-key")
        .with_base(format!("{}/api/v2", server.uri()));
    let games = client.search("portal2").await.unwrap();
    assert_eq!(games[0].id, 10);
    let art = client
        .art(10, ArtKind::Cover, &SgdbArtOptions::default())
        .await
        .unwrap();
    assert_eq!(art[0].thumb, "https://cdn.example/thumb/abc.jpg");
}

#[tokio::test]
async fn steam_text_query_params_and_parsing_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/appdetails"))
        .and(query_param("appids", "10"))
        .and(query_param("l", "portuguese"))
        .and(header("user-agent", "Mozilla/5.0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "10": {"success": true, "data": {
                "short_description": "<b>Curta</b>",
                "about_the_game": "<p>Longa</p>"
            }}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = SteamClient::new(fetcher(&server))
        .with_store_base(format!("{}/api/appdetails", server.uri()));
    let texts = client.textos("steam:10", "portuguese", "").await.unwrap();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0].texto, "Curta");
    assert_eq!(texts[1].texto, "Longa");
}

#[tokio::test]
async fn xbox_search_and_product_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v9.0/search"))
        .and(query_param("query", "Portal 2"))
        .and(query_param("market", "BR"))
        .and(query_param("locale", "pt-br"))
        .and(query_param("deviceFamily", "Windows.Desktop"))
        .and(header("user-agent", "Mozilla/5.0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Payload": {"SearchResults": [
                {"ProductId": "9A", "Title": "Portal 2"},
                {"ProductId": "9B", "Title": "Nada a ver"}
            ]}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v7.0/products"))
        .and(query_param("bigIds", "9A"))
        .and(query_param("market", "BR"))
        .and(query_param("languages", "pt-br"))
        .and(query_param("MS-CV", "x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Products": [{"LocalizedProperties": [{
                "ProductTitle": "Portal 2",
                "ProductDescription": "<p>Descrição</p>",
                "Images": [{"ImagePurpose": "Poster", "Uri": "//img/p.jpg", "Width": 720, "Height": 1080}]
            }]}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = XboxClient::new(fetcher(&server))
        .with_search_base(format!("{}/v9.0/search", server.uri()))
        .with_catalog_base(format!("{}/v7.0/products", server.uri()));
    let games = client.search("Portal 2", "BR", "pt-br").await.unwrap();
    assert_eq!(games.len(), 1);
    let product = client
        .product(&games[0].id, "BR", "pt-br")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(product.product_title, "Portal 2");
}

#[tokio::test]
async fn igdb_search_posts_json_and_fetches_detail_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/igdb/search"))
        .and(header("content-type", "application/json"))
        .and(header(
            "user-agent",
            "Arcadia (github.com/imperat-on/arcadia)",
        ))
        .and(body_json(json!({"SearchTerm": "Portal 2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{
                "id": 42,
                "name": "Portal 2",
                "platforms": [{"name": "PC"}],
                "first_release_date": 1_302_931_200i64
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/igdb/game/42"))
        .and(header(
            "user-agent",
            "Arcadia (github.com/imperat-on/arcadia)",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "name": "Portal 2",
                "summary": "Resumo",
                "storyline": "Enredo",
                "cover_expanded": {"image_id": "cover1"},
                "artworks_expanded": [],
                "screenshots_expanded": []
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = IgdbClient::new(fetcher(&server)).with_base(server.uri());
    let games = client
        .search(&IgdbSearch::new("Portal 2").with_platform("pc"))
        .await
        .unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].cover.as_ref().unwrap().image_id, "cover1");
}

#[tokio::test]
async fn hltb_end_to_end_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<script src="/_next/static/chunks/pages/_app-abc123.js"></script>"#,
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/_next/static/chunks/pages/_app-abc123.js"))
        .and(header("referer", format!("{}/", server.uri()).as_str()))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(r#"const x="/api/seek/"+ "abc123";"#),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/seek/abc123"))
        .and(header("content-type", "application/json"))
        .and(header("origin", server.uri().as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"game_name": "Portal 2", "comp_main": 5400, "comp_plus": 7200, "comp_100": 10800}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = HltbClient::new(fetcher(&server)).with_base(server.uri());
    let times = client.search("Portal 2").await.unwrap();
    assert_eq!(
        (times.main, times.main_extra, times.completionist),
        (90, 120, 180)
    );
}

#[tokio::test]
async fn psn_graphql_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/op"))
        .and(query_param("operationName", "getSearchResults"))
        .and(header(
            "apollographql-client-name",
            "@sie-ppr-web-store/app",
        ))
        .and(header("x-psn-store-locale-override", "en-US"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"universalSearch": {"results": [{
                "name": "Portal 2 Complete Edition",
                "media": [{"type": "IMAGE", "role": "MASTER", "url": "https://img/master.png"}]
            }]}}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = PsnClient::new(fetcher(&server)).with_graphql_base(format!("{}/op", server.uri()));
    let game = client
        .game_from_search("Portal 2 Complete")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        game.media_url("MASTER").as_deref(),
        Some("https://img/master.png")
    );
}

#[tokio::test]
async fn art_download_fetches_and_writes_the_content_type_extension() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art/no-extension"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/webp")
                .set_body_bytes(b"imagem".to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("steam_10-cover-1");
    let downloaded = download_to(
        fetcher(&server).as_ref(),
        &format!("{}/art/no-extension", server.uri()),
        &base,
    )
    .await
    .unwrap();
    assert_eq!(downloaded.path, dir.path().join("steam_10-cover-1.webp"));
    assert_eq!(std::fs::read(&downloaded.path).unwrap(), b"imagem");
    assert_eq!(downloaded.bytes, 6);
}

#[tokio::test]
async fn per_request_timeout_aborts_a_slow_local_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_string("tarde"),
        )
        .mount(&server)
        .await;

    let client = HttpClientFetcher::from_base_url(&server.uri()).unwrap();
    let error = client
        .fetch(
            arcadia_media::Request::get(format!("{}/slow", server.uri()))
                .timeout(Duration::from_millis(40)),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("timeout"), "erro: {error}");
}

#[tokio::test]
async fn rate_limit_is_a_typed_http_error_over_local_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/search/autocomplete/x"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .expect(1)
        .mount(&server)
        .await;

    let client =
        SteamGridDb::new(fetcher(&server), "k").with_base(format!("{}/api/v2", server.uri()));
    let error = client.search("x").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("HTTP 429 em {}/api/v2/search/autocomplete/x", server.uri())
    );
}
