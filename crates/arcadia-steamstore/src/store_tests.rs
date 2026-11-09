use super::*;
use crate::test_support::FixtureFetcher;
use arcadia_catalog::catalog::{CatalogClient, MemoryMirrorStore, NoAuth};
use arcadia_catalog::http::Method;
use arcadia_catalog::sources::ManualClock;
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(fetcher: Arc<FixtureFetcher>) -> StoreClient {
    StoreClient::new(fetcher, StoreConfig::default().with_language("pt-BR"))
}

fn steam_search(items: Value) -> Value {
    json!({ "items": items })
}

#[test]
fn language_mapping_matches_the_frozen_map() {
    assert_eq!(steam_language("pt-BR"), "portuguese");
    assert_eq!(steam_language("en-US"), "english");
    assert_eq!(steam_language("es-ES"), "spanish");
    assert_eq!(steam_language("fr-FR"), "english");
    assert_eq!(
        StoreConfig::default()
            .with_language("pt-BR")
            .steam_language(),
        "portuguese"
    );
    assert_eq!(StoreConfig::default().country_upper(), "BR");
}

#[test]
fn search_url_encodes_space_as_percent20_and_keeps_country_language_order() {
    let bases = StoreBases::default();
    assert_eq!(
        bases.search_url("Portal 2", "br", "portuguese"),
        "https://store.steampowered.com/api/storesearch/?term=Portal%202&cc=br&l=portuguese"
    );
    assert_eq!(
        bases.appdetails_url("10", "br", "english"),
        "https://store.steampowered.com/api/appdetails?appids=10&cc=br&l=english"
    );
    assert_eq!(
        bases.hubcap_library_url("Portal 2"),
        "https://hubcapmanifest.com/api/v1/library?search=Portal%202"
    );
    assert_eq!(
        bases.hubcap_manifest_url("440"),
        "https://hubcapmanifest.com/api/v1/manifest/440"
    );
    assert_eq!(bases.ryuu_url("440"), "http://167.235.229.108/440");
    assert!(bases.sushi_url("440").ends_with("/440.zip"));
    assert_eq!(
        bases.twenty_two_url("440"),
        "https://api.twentytwocloud.com/download?appid=440"
    );
}

#[tokio::test]
async fn suggest_short_query_does_not_touch_network() {
    let fetcher = Arc::new(FixtureFetcher::new());
    let response = client(fetcher.clone()).suggest("a").await;
    assert!(response.ok);
    assert_eq!(response.jogos.unwrap(), Vec::<StoreGame>::new());
    assert!(fetcher.calls().is_empty());
}

#[tokio::test]
async fn suggest_parses_items_cover_fallback_and_caps_at_eight() {
    let items = (0..12)
        .map(|id| json!({"id": id, "name": format!("Jogo {id}")}))
        .collect::<Vec<_>>();
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "store.steampowered.com/api/storesearch",
        steam_search(Value::Array(items)),
    ));
    let response = client(fetcher.clone()).suggest("jogo").await;
    let games = response.jogos.unwrap();
    assert_eq!(games.len(), SUGGEST_LIMIT);
    assert_eq!(games[0].appid, "0");
    assert_eq!(
        games[0].cover,
        "https://cdn.cloudflare.steamstatic.com/steam/apps/0/header.jpg"
    );
    assert_eq!(
        fetcher.urls()[0],
        "https://store.steampowered.com/api/storesearch/?term=jogo&cc=br&l=portuguese"
    );
}

#[tokio::test]
async fn suggest_filters_dlc_from_the_local_item_cache_and_reuses_cache() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .json(
                "IStoreBrowseService/GetItems",
                json!({"response": {"store_items": [
                    {"appid": 10, "type": 0},
                    {"appid": 20, "type": 4}
                ]}}),
            )
            .json(
                "store.steampowered.com/api/storesearch",
                steam_search(json!([
                    {"id": 10, "name": "Portal"},
                    {"id": 20, "name": "Portal DLC"}
                ])),
            ),
    );
    let store = client(fetcher.clone());
    store.items_for(&["10".to_string(), "20".to_string()]).await;

    let response = store.suggest("portal").await;
    let games = response.jogos.unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].appid, "10");

    let search_calls = fetcher
        .urls()
        .iter()
        .filter(|url| url.contains("storesearch"))
        .count();
    let cached = store.suggest("portal").await;
    assert_eq!(cached.cache, Some(true));
    assert_eq!(cached.jogos.unwrap().len(), 1);
    assert_eq!(
        fetcher
            .urls()
            .iter()
            .filter(|url| url.contains("storesearch"))
            .count(),
        search_calls
    );
}

#[tokio::test]
async fn search_merges_hubcap_and_steam_with_relevance_and_accessory_order() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .json(
                "hubcapmanifest.com/api/v1/library",
                json!({"games": [
                    {"game_id": 400, "game_name": "Portal Soundtrack", "header_image": "h400"},
                    {"game_id": 730, "game_name": "Portal 2", "header_image": "h730", "manifest_available": true}
                ]}),
            )
            .json(
                "store.steampowered.com/api/storesearch",
                steam_search(json!([
                    {"id": 10, "name": "Portal", "tiny_image": "t10", "price": {"currency": "BRL", "final": 0}},
                    {"id": 730, "name": "Portal 2", "tiny_image": "t730"},
                    {"id": 30, "name": "Portal Stories: Mel"}
                ])),
            ),
    );
    let store = StoreClient::new(
        fetcher.clone(),
        StoreConfig::default()
            .with_language("pt-BR")
            .with_hubcap_key(Some("chave-secreta")),
    );
    let response = store.search("Portal").await;
    assert!(response.ok);
    assert_eq!(response.fonte.as_deref(), Some("multi"));
    assert_eq!(response.avisos.as_deref(), Some(&[][..]));
    let games = response.jogos.unwrap();
    assert_eq!(
        games
            .iter()
            .map(|game| game.appid.as_str())
            .collect::<Vec<_>>(),
        vec!["10", "730", "30", "400"]
    );
    assert_eq!(games[0].preco, "Gratuito");
    assert!(games[1].manifest);
    assert_eq!(games[1].fontes, vec!["Morrenus"]);
    assert!(!games[3].manifest);

    let hubcap_call = fetcher
        .calls()
        .into_iter()
        .find(|request| request.url.contains("hubcapmanifest"))
        .unwrap();
    assert!(!hubcap_call.url.contains("chave-secreta"));
    assert!(hubcap_call
        .headers
        .iter()
        .any(|(name, value)| name == "Authorization" && value == "Bearer chave-secreta"));
    assert!(hubcap_call
        .headers
        .iter()
        .any(|(name, value)| name == "User-Agent" && value == "arcadia"));
}

#[tokio::test]
async fn search_drops_non_game_types_and_enriches_capa_heroi_from_items() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .json(
                "store.steampowered.com/api/storesearch",
                steam_search(json!([
                    {"id": 10, "name": "Portal"},
                    {"id": 20, "name": "Portal DLC"}
                ])),
            )
            .json(
                "IStoreBrowseService/GetItems",
                json!({"response": {"store_items": [
                    {"appid": 10, "type": 0, "assets": {
                        "asset_url_format": "steam/apps/10/${FILENAME}?",
                        "library_capsule": "library_600x900.jpg",
                        "library_hero": "library_hero.jpg",
                        "icon": "icon.jpg"
                    }},
                    {"appid": 20, "type": 4, "assets": {"asset_url_format": "x/${FILENAME}"}}
                ]}}),
            ),
    );
    let response = client(fetcher).search("Portal").await;
    let games = response.jogos.unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].appid, "10");
    assert_eq!(
        games[0].capa.as_deref(),
        Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/10/library_600x900.jpg?")
    );
    assert_eq!(
        games[0].heroi.as_deref(),
        Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/10/library_hero.jpg?")
    );
}

#[tokio::test]
async fn search_failure_returns_both_source_errors() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .text("store.steampowered.com/api/storesearch", 503, "")
            .text("hubcapmanifest.com/api/v1/library", 500, ""),
    );
    let store = StoreClient::new(fetcher, StoreConfig::default().with_hubcap_key(Some("k")));
    let response = store.search("nada").await;
    assert!(!response.ok);
    let error = response.error.unwrap();
    assert!(error.contains("Hubcap: HTTP 500"), "{error}");
    assert!(error.contains("Steam: HTTP 503"), "{error}");
}

#[tokio::test]
async fn search_cache_respects_the_five_minute_ttl() {
    let clock = Arc::new(ManualClock::new(1_000));
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "store.steampowered.com/api/storesearch",
        steam_search(json!([{"id": 10, "name": "Portal"}])),
    ));
    let store = client(fetcher.clone()).with_clock(clock.clone());
    assert!(store.search("Portal").await.ok);
    let first = store.search("Portal").await;
    assert_eq!(first.cache, Some(true));
    assert_eq!(
        fetcher
            .urls()
            .iter()
            .filter(|url| url.contains("storesearch"))
            .count(),
        1
    );
    clock.advance(SEARCH_TTL_MS + 1);
    assert!(store.search("Portal").await.ok);
    assert_eq!(
        fetcher
            .urls()
            .iter()
            .filter(|url| url.contains("storesearch"))
            .count(),
        2
    );
}

#[tokio::test]
async fn items_cache_stores_negative_verdicts_and_refetches_after_ttl() {
    let clock = Arc::new(ManualClock::new(10));
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "IStoreBrowseService/GetItems",
        json!({"response": {"store_items": []}}),
    ));
    let store = client(fetcher.clone()).with_clock(clock.clone());
    let first = store.items_for(&["10".to_string()]).await;
    assert!(first.respondidos.contains("10"));
    assert!(first.mapa.is_empty());
    let second = store.items_for(&["10".to_string()]).await;
    assert!(second.respondidos.contains("10"));
    assert_eq!(
        fetcher
            .urls()
            .iter()
            .filter(|url| url.contains("GetItems"))
            .count(),
        1
    );
    clock.advance(ITEMS_TTL_MS + 1);
    store.items_for(&["10".to_string()]).await;
    assert_eq!(
        fetcher
            .urls()
            .iter()
            .filter(|url| url.contains("GetItems"))
            .count(),
        2
    );
}

#[tokio::test]
async fn items_request_carries_language_country_realm_and_include_assets() {
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "IStoreBrowseService/GetItems",
        json!({"response": {"store_items": []}}),
    ));
    let store = client(fetcher.clone());
    store.items_for(&["10".to_string()]).await;
    let url = fetcher
        .urls()
        .into_iter()
        .find(|url| url.contains("GetItems"))
        .unwrap();
    assert!(url
        .starts_with("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json="));
    let encoded = url.split("input_json=").nth(1).unwrap();
    let decoded = percent_decode(encoded);
    let value: Value = serde_json::from_str(&decoded).unwrap();
    assert_eq!(value["ids"][0]["appid"], 10);
    assert_eq!(value["context"]["language"], "portuguese");
    assert_eq!(value["context"]["country_code"], "BR");
    assert_eq!(value["context"]["steam_realm"], 1);
    assert_eq!(value["data_request"]["include_assets"], true);
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap()
}

#[tokio::test]
async fn appdetails_parses_sysinfo_shape_and_prefers_the_richer_about() {
    let screenshots = (0..13)
        .map(|id| {
            json!({"id": id, "path_thumbnail": format!("t{id}"), "path_full": format!("f{id}")})
        })
        .collect::<Vec<_>>();
    let movies = (0..7)
        .map(|id| {
            json!({
                "id": id,
                "name": format!("Trailer {id}"),
                "thumbnail": format!("thumb{id}"),
                "mp4": {"max": format!("mp4{id}")},
                "webm": {"480": format!("webm{id}")},
                "hls_h264": format!("hls{id}")
            })
        })
        .collect::<Vec<_>>();
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "store.steampowered.com/api/appdetails",
        json!({"10": {"success": true, "data": {
            "short_description": "curta",
            "about_the_game": "<p>about</p>",
            "detailed_description": "<p>detalhada</p><img src=\"1\"><img src=\"2\">",
            "pc_requirements": {"minimum": "min", "recommended": "rec"},
            "publishers": ["Valve"],
            "developers": ["Valve"],
            "release_date": {"date": "10 Oct, 2007"},
            "controller_support": "full",
            "supported_languages": "English",
            "header_image": "header",
            "background_raw": "bg",
            "screenshots": screenshots,
            "movies": movies
        }}}),
    ));
    let store = client(fetcher.clone());
    let envelope = store.app_details_response("10").await;
    assert!(envelope.ok);
    let info = envelope.info.clone().unwrap();
    assert_eq!(info.appid, "10");
    assert_eq!(info.short_description, "curta");
    assert_eq!(info.about, "<p>detalhada</p><img src=\"1\"><img src=\"2\">");
    assert_eq!(info.req_min, "min");
    assert_eq!(info.req_rec, "rec");
    assert_eq!(info.publishers, vec!["Valve"]);
    assert_eq!(info.release_date, "10 Oct, 2007");
    assert_eq!(info.background, "bg");
    assert_eq!(info.screenshots.len(), 12);
    assert_eq!(info.movies.len(), 6);
    assert_eq!(info.movies[0].mp4, "mp40");
    assert_eq!(info.movies[0].webm, "webm0");
    assert_eq!(info.movies[0].hls, "hls0");
    assert_eq!(
        fetcher.urls()[0],
        "https://store.steampowered.com/api/appdetails?appids=10&cc=br&l=portuguese"
    );
    let value = serde_json::to_value(&envelope).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["info"]["controller_support"], "full");
}

#[tokio::test]
async fn appdetails_without_success_is_an_error_envelope() {
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "store.steampowered.com/api/appdetails",
        json!({"10": {"success": false}}),
    ));
    let response = client(fetcher).app_details_response("10").await;
    assert!(!response.ok);
    assert_eq!(response.error.as_deref(), Some("appdetails sem dados"));
}

#[tokio::test]
async fn appdetails_rejects_non_numeric_appid_without_network() {
    let fetcher = Arc::new(FixtureFetcher::new());
    let response = client(fetcher.clone())
        .app_details_response("10&cc=us")
        .await;
    assert!(!response.ok);
    assert!(fetcher.calls().is_empty());
}

#[test]
fn price_formatting_and_empty_values() {
    assert_eq!(format_price(0.0, "BRL"), "Gratuito");
    assert_eq!(format_price(123_456.0, "BRL"), "R$ 1.234,56");
    assert_eq!(format_price(123_456.0, "USD"), "$1,234.56");
    assert_eq!(format_price(123_456.0, "EUR"), "€ 1.234,56");
    assert_eq!(format_price(100.0, ""), "");
    assert_eq!(
        search_price(Some(&json!({"currency": "BRL", "final": 500}))),
        "R$ 5,00"
    );
    assert_eq!(search_price(None), "");
}

#[test]
fn location_matching_extracts_appids_from_json_values() {
    assert!(valid_appid("730"));
    assert!(!valid_appid(""));
    assert!(!valid_appid("73a"));
    assert!(!valid_appid("../730"));
    assert_eq!(extract_appid(&json!(730)), Some("730".to_string()));
    assert_eq!(extract_appid(&json!("730")), Some("730".to_string()));
    assert_eq!(extract_appid(&json!("steam:730")), Some("730".to_string()));
    assert_eq!(
        extract_appid(&json!("  steam:730  ")),
        Some("730".to_string())
    );
    assert_eq!(extract_appid(&json!("")), None);
    assert_eq!(extract_appid(&json!(null)), None);
    assert_eq!(extract_appid(&json!({"appid": 1})), None);
}

#[test]
fn relevance_title_matching_and_accessory_ordering() {
    assert_eq!(relevance("Portal", "portal"), 0);
    assert_eq!(relevance("Portal 2", "portal"), 1);
    assert_eq!(relevance("The Portal", "portal"), 2);
    assert_eq!(relevance("My Portal Game", "portal"), 2);
    assert_eq!(relevance("Portalistic", "portal"), 1);
    assert_eq!(relevance("Myportal", "portal"), 3);
    assert_eq!(relevance("Other", "portal"), 4);
    assert!(is_accessory("Portal 2 Soundtrack"));
    assert!(is_accessory("Game Demo"));
    assert!(!is_accessory("Portal 2"));

    let mut games = vec![
        StoreGame {
            manifest: false,
            ..StoreGame::new("1", "Portal Soundtrack")
        },
        StoreGame {
            manifest: false,
            ..StoreGame::new("2", "Portalistic")
        },
        StoreGame {
            manifest: true,
            ..StoreGame::new("3", "Portal 2")
        },
        StoreGame {
            manifest: false,
            ..StoreGame::new("4", "Portal")
        },
    ];
    sort_games(&mut games, "Portal");
    assert_eq!(
        games
            .iter()
            .map(|game| game.appid.as_str())
            .collect::<Vec<_>>(),
        vec!["4", "3", "2", "1"]
    );
}

#[test]
fn strip_html_matches_the_metadata_helper() {
    assert_eq!(
        strip_html("<p>Linha 1</p><p>Linha 2</p><br/>Fim &amp; &quot;ok&quot;&nbsp;!"),
        "Linha 1\nLinha 2\n\nFim & \"ok\" !"
    );
    assert_eq!(strip_html("a\n\n\n\nb"), "a\n\nb");
    assert_eq!(strip_html("<b>negrito</b>"), "negrito");
}

#[tokio::test]
async fn hubcap_key_validation_separates_invalid_key_from_outage() {
    let ok =
        Arc::new(FixtureFetcher::new().text("hubcapmanifest.com/api/v1/manifest/440", 200, "PK"));
    let result = client(ok.clone()).validate_hubcap_key("  k  ").await;
    assert!(result.ok);
    assert_eq!(result.status, Some(200));
    assert_eq!(
        ok.urls()[0],
        "https://hubcapmanifest.com/api/v1/manifest/440"
    );
    assert!(ok.calls()[0]
        .headers
        .iter()
        .any(|(name, value)| name == "Authorization" && value == "Bearer k"));

    let invalid = Arc::new(FixtureFetcher::new().text("hubcapmanifest.com", 401, ""));
    let result = client(invalid).validate_hubcap_key("k").await;
    assert!(!result.ok);
    assert_eq!(result.motivo.as_deref(), Some("chave_invalida"));

    let outage = Arc::new(FixtureFetcher::new().text("hubcapmanifest.com", 502, ""));
    let result = client(outage).validate_hubcap_key("k").await;
    assert_eq!(result.motivo.as_deref(), Some("http_502"));

    let network = Arc::new(FixtureFetcher::new().error("hubcapmanifest.com", "dns"));
    let result = client(network).validate_hubcap_key("k").await;
    assert_eq!(result.motivo.as_deref(), Some("rede"));
    assert_eq!(result.erro.as_deref(), Some("dns"));

    let none = Arc::new(FixtureFetcher::new());
    assert_eq!(
        client(none.clone())
            .validate_hubcap_key(" ")
            .await
            .motivo
            .as_deref(),
        Some("sem_chave")
    );
    assert!(none.calls().is_empty());
}

#[tokio::test]
async fn recent_uses_steamspy_ccu_order_and_paginates() {
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "steamspy.com/api.php?request=top100in2weeks",
        json!({
            "1": {"appid": 1, "name": "A", "ccu": 50},
            "2": {"appid": 2, "name": "B", "ccu": 100},
            "3": {"appid": 3, "name": "C", "ccu": 10}
        }),
    ));
    let store = client(fetcher.clone());
    let response = store.recent(None, 1, 1).await;
    assert!(response.ok);
    let games = response.jogos.unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].appid, "1");
    assert_eq!(response.total, Some(3));
    assert_eq!(response.offset, Some(1));
    assert_eq!(response.has_more, Some(true));
    assert_eq!(response.next_offset, Some(json!(2)));
    assert_eq!(
        games[0].cover,
        "https://cdn.akamai.steamstatic.com/steam/apps/1/header.jpg"
    );
    let second = store.recent(None, 1, 1).await;
    assert_eq!(second.cache, Some(true));
    assert_eq!(
        fetcher
            .urls()
            .iter()
            .filter(|url| url.contains("steamspy"))
            .count(),
        1
    );
}

#[tokio::test]
async fn recent_all_reads_the_catalog_page_without_reslicing() {
    let fetcher = Arc::new(FixtureFetcher::new().json(
        "/catalog/v1/catalog?offset=2&limite=2",
        json!({"itens": [
            {"appid": 10, "title": "A", "cover": "a"},
            {"appid": 20, "title": "B", "cover": "b"}
        ], "total": 10}),
    ));
    let catalog = CatalogClient::new(
        fetcher.clone(),
        Arc::new(MemoryMirrorStore::new()),
        Arc::new(NoAuth),
        "http://catalog.test",
    );
    let store = client(fetcher).with_catalog(catalog);
    let response = store.recent(Some("all"), 2, 2).await;
    assert!(response.ok);
    let games = response.jogos.unwrap();
    assert_eq!(games.len(), 2);
    assert_eq!(games[0].appid, "10");
    assert_eq!(response.total, Some(10));
    assert_eq!(response.offset, Some(2));
    assert_eq!(response.has_more, Some(true));
    assert_eq!(response.next_offset, Some(json!(4)));
}

#[tokio::test]
async fn recent_genre_reads_catalog_then_falls_back_to_steamspy() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .json(
                "/catalog/v1/genre?lista=top100forever",
                json!({"data": {"completa": [
                    {"appid": 1, "title": "A"},
                    {"appid": 2, "title": "B"},
                    {"appid": 3, "title": "C"}
                ]}}),
            )
            .json(
                "steamspy.com/api.php?request=top100forever",
                json!({"9": {"appid": 9, "name": "Z", "ccu": 1}}),
            ),
    );
    let catalog = CatalogClient::new(
        fetcher.clone(),
        Arc::new(MemoryMirrorStore::new()),
        Arc::new(NoAuth),
        "http://catalog.test",
    );
    let store = client(fetcher.clone()).with_catalog(catalog);
    let response = store.recent(Some("top100forever"), 2, 0).await;
    assert!(response.ok);
    assert_eq!(
        response
            .jogos
            .unwrap()
            .iter()
            .map(|game| game.appid.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2"]
    );
    assert!(fetcher.urls().iter().all(|url| !url.contains("steamspy")));
}

#[tokio::test]
async fn storesearch_contract_over_local_http_with_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/storesearch/"))
        .and(query_param("term", "Portal 2"))
        .and(query_param("cc", "br"))
        .and(query_param("l", "portuguese"))
        .and(header("user-agent", "arcadia"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id": 10, "name": "Portal 2", "tiny_image": "t"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let bases = StoreBases {
        search: format!("{}/api/storesearch/", server.uri()),
        ..StoreBases::default()
    };
    let store = StoreClient::new(
        Arc::new(arcadia_catalog::http::HttpClientFetcher::from_base_url(&server.uri()).unwrap()),
        StoreConfig::default().with_language("pt-BR"),
    )
    .with_bases(bases);
    let response = store.suggest("Portal 2").await;
    let games = response.jogos.unwrap();
    assert_eq!(games[0].appid, "10");
    assert_eq!(games[0].cover, "t");
    server.verify().await;
}

#[tokio::test]
async fn search_probes_sushi_and_ryuu_when_catalog_is_absent() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .json(
                "store.steampowered.com/api/storesearch",
                steam_search(json!([{"id": 10, "name": "Portal"}])),
            )
            .json(
                "api.github.com/repos/sushi-dev55-alt/sushitools-games-repo-alt/git/trees/main",
                json!({"tree": [{"path": "10.zip"}, {"path": "20.zip"}], "truncated": false}),
            )
            .method(Method::Head, "167.235.229.108/10", 200, ""),
    );
    let store = client(fetcher.clone()).with_availability_probe(true);
    let response = store.search("Portal").await;
    let games = response.jogos.unwrap();
    assert_eq!(games[0].fontes, vec!["Sushi", "Ryuu"]);
    assert!(games[0].manifest);
    assert!(fetcher.calls().iter().any(|request| {
        request.method == Method::Head && request.url == "http://167.235.229.108/10"
    }));
}

#[tokio::test]
async fn warm_hits_the_frozen_warmup_url() {
    let fetcher =
        Arc::new(FixtureFetcher::new().json("store.steampowered.com/api/storesearch", json!({})));
    client(fetcher.clone()).warm().await.unwrap();
    assert_eq!(
        fetcher.urls()[0],
        "https://store.steampowered.com/api/storesearch/?term=a&cc=br"
    );
}
