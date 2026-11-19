use super::*;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const HTML: &str = include_str!("../../tests/fixtures/steam-achievements.html");
const SSR_HTML: &str = include_str!("../../tests/fixtures/steam-achievements-ssr.html");
const AGEGATE: &str = include_str!("../../tests/fixtures/steam-agegate.html");
const PERCENTAGES: &str = include_str!("../../tests/fixtures/steam-percentages.json");

struct FixedClock(i64);

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

struct FixtureHttp(Mutex<VecDeque<(String, Result<HttpResponse, String>)>>);

impl FixtureHttp {
    fn new(responses: Vec<(String, &str)>) -> Self {
        Self(Mutex::new(
            responses
                .into_iter()
                .map(|(url, body)| {
                    (
                        url,
                        Ok(HttpResponse {
                            status: 200,
                            body: body.into(),
                        }),
                    )
                })
                .collect(),
        ))
    }

    fn assert_consumed(&self) {
        assert!(self.0.lock().unwrap().is_empty());
    }
}

#[async_trait]
impl AchievementsHttp for FixtureHttp {
    async fn get(&self, url: &str, timeout: Duration) -> Result<HttpResponse, String> {
        assert!(timeout <= FETCH_TIMEOUT && !timeout.is_zero());
        let (expected, response) = self.0.lock().unwrap().pop_front().expect("HTTP inesperado");
        assert_eq!(url, expected);
        response
    }
}

struct CacheDir(PathBuf);

impl CacheDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-steam-page-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> PathBuf {
        self.0.join("achievements_store.json")
    }
}

impl Drop for CacheDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn parser_reads_legacy_fixture_and_unescapes_text() {
    let items = parse_achievements_html(HTML);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["apiname"], "PLAY_CS2");
    assert_eq!(items[0]["title"], "Counter-Strike & \"CS2\"");
    assert_eq!(items[0]["desc"], "Comece <agora> 'mesmo'.");
    assert_eq!(items[0]["percent"], 80.8);
    assert_eq!(items[1]["percent"], 72.7);
    assert_eq!(items[2]["desc"], "");
    assert!(items[2]["percent"].is_null());
    assert!(parse_achievements_html(AGEGATE).is_empty());
    assert!(parse_achievements_html("<html>sem conquistas</html>").is_empty());
}

#[test]
fn parser_reads_current_ssr_schema_including_hidden_and_all_groups() {
    let items = parse_achievements_html(SSR_HTML);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["apiname"], "Achievement1");
    assert_eq!(items[1]["apiname"], "Achievement2");
    assert_eq!(items[0]["title"], "Na Lama Outra Vez");
    assert_eq!(items[0]["percent"], 72.69999694824219);
    assert!(items[0]["icon"]
        .as_str()
        .unwrap()
        .contains("/1174180/3f5b6b"));
    assert!(items[0]["icongray"]
        .as_str()
        .unwrap()
        .contains("/1174180/568f85"));
    assert_eq!(items[1]["icon"], items[1]["icongray"]);
    assert!(parse_achievements_html(r#"<script id="valve-ssr-data">invalid</script>"#).is_empty());
}

#[test]
fn percentage_matching_is_unique_on_both_sides_and_not_positional() {
    let items = vec![
        json!({"apiname":"", "percent":72.7}),
        json!({"apiname":"HTML_NAME", "percent":1}),
        json!({"apiname":"", "percent":2}),
        json!({"apiname":"", "percent":2}),
        json!({"apiname":"", "percent":3}),
        json!({"apiname":"", "percent":null}),
    ];
    let api = vec![
        json!({"name":"OTHER", "percent":1}),
        json!({"name":"A", "percent":72.69999694824219}),
        json!({"name":"DUP_HTML", "percent":2}),
        json!({"name":"DUP_API_1", "percent":3}),
        json!({"name":"DUP_API_2", "percent":3}),
    ];
    let matched = apinames_by_percent(&items, &api);
    let names: Vec<_> = matched
        .iter()
        .map(|item| item["apiname"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["A", "HTML_NAME", "ach_03", "ach_04", "ach_05", "ach_06"]
    );
}

#[test]
fn percentage_keys_match_javascript_to_fixed_rounding() {
    for (number, expected) in [
        (1.25, "1.3"),
        (1.75, "1.8"),
        (2.55, "2.5"),
        (72.69999694824219, "72.7"),
        (0.0, "0.0"),
    ] {
        assert_eq!(percent_key(&json!(number)).as_deref(), Some(expected));
    }
    assert_eq!(percent_key(&Value::Null).as_deref(), Some("0.0"));
}

#[tokio::test]
async fn fetch_handles_agegate_api_names_cache_and_ttl_without_network() {
    let dir = CacheDir::new("agegate");
    let http = FixtureHttp::new(vec![
        (fetch_consent_url("1174180"), AGEGATE),
        (
            format!("{}&agree_to_agegate=1", fetch_consent_url("1174180")),
            HTML,
        ),
        (apinames_api_url("1174180"), PERCENTAGES),
        (fetch_consent_url("1174180"), SSR_HTML),
    ]);
    let now = 2 * STORE_TTL_MS;
    let first = fetch_achievements_for_app("1174180", &dir.path(), &http, &FixedClock(now))
        .await
        .unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(first[1]["apiname"], "Achievement1");
    assert!(first.iter().all(|item| item["achieved"] == false
        && item["unlock"] == 0
        && item["block"].is_null()
        && item["bit"].is_null()));
    assert_eq!(
        AchievementsStore::load(dir.path())
            .entry("1174180")
            .unwrap()["at"],
        now
    );
    let cached = fetch_achievements_for_app(
        "1174180",
        &dir.path(),
        &http,
        &FixedClock(now + STORE_TTL_MS - 1),
    )
    .await
    .unwrap();
    assert_eq!(cached, first);
    let refreshed = fetch_achievements_for_app(
        "1174180",
        &dir.path(),
        &http,
        &FixedClock(now + STORE_TTL_MS),
    )
    .await
    .unwrap();
    assert_eq!(refreshed.len(), 2);
    http.assert_consumed();
}

#[tokio::test]
async fn synthetic_only_cache_is_refetched_and_other_cache_entries_survive() {
    let dir = CacheDir::new("synthetic");
    let mut store = AchievementsStore::from_value(json!({
        "1174180":{"at":1000,"custom":42,"items":[{"apiname":"ach_01"}]},
        "730":{"at":1,"items":[{"apiname":"PLAY_CS2"}]}
    }));
    store.save_atomic(dir.path()).unwrap();
    let http = FixtureHttp::new(vec![(fetch_consent_url("1174180"), SSR_HTML)]);
    let items = fetch_achievements_for_app("1174180", &dir.path(), &http, &FixedClock(1001))
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    store = AchievementsStore::load(dir.path());
    assert_eq!(store.entry("1174180").unwrap()["custom"], 42);
    assert_eq!(store.items("730")[0]["apiname"], "PLAY_CS2");
    http.assert_consumed();
}

#[tokio::test]
async fn public_cache_never_supplies_unlock_state_and_ignores_invalid_items() {
    let dir = CacheDir::new("cache-state");
    let store = AchievementsStore::from_value(json!({
        "1174180":{"at":1000,"items":[null, 7,
            {"apiname":"Achievement1","achieved":true,"unlock":123}]}
    }));
    store.save_atomic(dir.path()).unwrap();
    let http = FixtureHttp::new(Vec::new());
    let items = fetch_achievements_for_app("1174180", &dir.path(), &http, &FixedClock(1001))
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["achieved"], false);
    assert_eq!(items[0]["unlock"], 0);
    http.assert_consumed();
}

#[tokio::test]
async fn empty_page_does_not_poison_cache_and_api_failure_uses_synthetic_names() {
    let dir = CacheDir::new("empty");
    let http = FixtureHttp::new(vec![(fetch_consent_url("1174180"), "<html></html>")]);
    assert!(
        fetch_achievements_for_app("1174180", &dir.path(), &http, &FixedClock(1))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!dir.path().exists());
    let http = FixtureHttp::new(vec![(fetch_consent_url("1174180"), HTML)]);
    http.0
        .lock()
        .unwrap()
        .push_back((apinames_api_url("1174180"), Err("offline".into())));
    let items = fetch_achievements_for_app("1174180", &dir.path(), &http, &FixedClock(1))
        .await
        .unwrap();
    assert_eq!(items[1]["apiname"], "ach_02");
    assert_eq!(items[2]["percent"], 0);
    http.assert_consumed();
}

#[test]
fn catalog_merge_preserves_local_unlock_fields_aliases_and_unmatched_items() {
    let local = vec![
        json!({"apiname":"LOCAL", "aliases":["Achievement1"],
        "title":"Nome local", "achieved":true, "unlock":123, "localUnlock":true,
        "block":4,"bit":2,"unknown":{"keep":true}}),
        json!({"apiname":"UNMATCHED", "achieved":true,"unlock":99}),
    ];
    let catalog = vec![
        json!({"apiname":"Achievement1", "title":"Nome remoto", "achieved":false}),
        json!({"apiname":"Achievement2", "title":"Só um Arranhão", "achieved":true,"unlock":456}),
    ];
    let items = merge_catalog_items(catalog, &local);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["apiname"], "LOCAL");
    assert_eq!(items[0]["achieved"], true);
    assert_eq!(items[0]["unlock"], 123);
    assert_eq!(items[0]["block"], 4);
    assert_eq!(items[0]["unknown"], json!({"keep":true}));
    assert_eq!(items[1]["achieved"], false);
    assert_eq!(items[1]["unlock"], 0);
    assert_eq!(items[2], local[1]);
}
