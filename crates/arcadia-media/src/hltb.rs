//! HowLongToBeat: tempos via scrape do site, com cache local de 30 dias.
//!
//! O fluxo é o do cliente congelado:
//! 1. baixa a home e descobre o bundle `_app-*.js` do Next;
//! 2. extrai o hash de `/api/seek/<hash>` do bundle (três padrões);
//! 3. `POST /api/seek/{hash}` com o corpo de busca;
//! 4. escolhe o jogo de nome normalizado igual ou o primeiro;
//! 5. converte segundos em minutos arredondados e grava o cache.
//!
//! Qualquer erro devolve `None` (falha silenciosa), como `hltbBuscar`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cache::{lock, Clock, SystemClock};
use crate::error::{MediaError, Result};
use crate::http::{value_number, value_str, Fetcher, Request};

pub const HLTB_BASE: &str = "https://howlongtobeat.com";
pub const HLTB_CACHE_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;
pub const HLTB_TIMEOUT: Duration = Duration::from_secs(15);
pub const HLTB_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

/// Hashes de reserva do JS (vazio no oráculo; a extração runtime é o caminho).
pub const SEEK_HASHES_FALLBACK: &[&str] = &[];

/// Tempos em horas inteiras, no formato do cache/UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HltbTimes {
    #[serde(default)]
    pub main: i64,
    #[serde(default, rename = "mainExtra")]
    pub main_extra: i64,
    #[serde(default)]
    pub completionist: i64,
    #[serde(default)]
    pub ts: u64,
}

/// `normalizar` do HLTB: minúsculas, runs de não-alfanuméricos viram UM
/// espaço, pontas aparadas. Não remove acentos (diferente de `normalizaTitulo`).
pub fn normalize_hltb_title(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for character in value.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(character);
        } else {
            pending_space = true;
        }
    }
    out
}

/// Corpo exato de `seek` (exportado para testes de contrato).
pub fn seek_body(title: &str) -> Value {
    let normalized = normalize_hltb_title(title);
    let terms: Vec<String> = normalized
        .split(' ')
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect();
    json!({
        "searchType": "games",
        "searchTerms": terms,
        "searchPage": 1,
        "size": 5,
        "searchOptions": {
            "games": {
                "userId": 0,
                "platform": "",
                "sortCategory": "popular",
                "rangeCategory": "main",
                "rangeTime": {"min": null, "max": null},
                "gameplay": {"perspective": "", "flow": "", "genre": ""},
                "modifier": ""
            },
            "users": {"sortCategory": "postcount"},
            "filter": "",
            "sort": 0,
            "randomizer": 0
        }
    })
}

/// `descobrirHash`: devolve `None` quando o bundle não está na página ou não
/// contém nenhum dos padrões conhecidos.
pub async fn discover_hash(fetcher: &dyn Fetcher) -> Result<Option<String>> {
    discover_hash_with_base(fetcher, HLTB_BASE).await
}

/// Igual a [`discover_hash`], com base configurável.
pub async fn discover_hash_with_base(fetcher: &dyn Fetcher, base: &str) -> Result<Option<String>> {
    static BUNDLE: OnceLock<Regex> = OnceLock::new();
    static PATTERN_1: OnceLock<Regex> = OnceLock::new();
    static PATTERN_2: OnceLock<Regex> = OnceLock::new();
    static PATTERN_3: OnceLock<Regex> = OnceLock::new();

    let base = base.trim_end_matches('/');
    let home = fetcher
        .fetch(Request::get(format!("{base}/")).header("User-Agent", HLTB_UA))
        .await?;
    let html = home.text_body();
    let bundle = BUNDLE.get_or_init(|| {
        Regex::new(r"(?i)_next/static/chunks/pages/(_app-[a-f0-9]+\.js)")
            .expect("regex do bundle HLTB")
    });
    let Some(capture) = bundle.captures(&html) else {
        return Ok(None);
    };
    let bundle_url = format!("{base}/_next/static/chunks/pages/{}", &capture[1]);
    let bundle = fetcher
        .fetch(
            Request::get(bundle_url)
                .header("User-Agent", HLTB_UA)
                .header("Referer", format!("{base}/")),
        )
        .await?;
    let javascript = bundle.text_body();

    let patterns: [&OnceLock<Regex>; 3] = [&PATTERN_1, &PATTERN_2, &PATTERN_3];
    let sources = [
        r#"(?i)["']/api/seek/["']\s*\+\s*["']([a-f0-9]+)["']"#,
        r#"(?i)["']/api/seek/["']\.concat\(\s*["']([a-f0-9]+)["']"#,
        r#"(?i)api/seek/([a-f0-9]{20,})"#,
    ];
    for (slot, source) in patterns.iter().zip(sources) {
        let regex = slot.get_or_init(|| Regex::new(source).expect("regex de hash HLTB"));
        if let Some(capture) = regex.captures(&javascript) {
            return Ok(Some(capture[1].to_string()));
        }
    }
    Ok(None)
}

/// `seek`: `POST /api/seek/{hash}` com timeout de 15 s.
pub async fn seek(fetcher: &dyn Fetcher, hash: &str, title: &str) -> Result<Value> {
    seek_with_base(fetcher, HLTB_BASE, hash, title).await
}

/// Igual a [`seek`], com base configurável.
pub async fn seek_with_base(
    fetcher: &dyn Fetcher,
    base: &str,
    hash: &str,
    title: &str,
) -> Result<Value> {
    let base = base.trim_end_matches('/');
    let body = serde_json::to_vec(&seek_body(title))
        .map_err(|error| MediaError::decode(error.to_string()))?;
    let response = fetcher
        .fetch(
            Request::post(format!("{base}/api/seek/{hash}"))
                .header("User-Agent", HLTB_UA)
                .header("Referer", format!("{base}/"))
                .header("Origin", base)
                .header("Content-Type", "application/json")
                .body(body)
                .timeout(HLTB_TIMEOUT),
        )
        .await?;
    if !response.is_success() {
        return Err(MediaError::HltbHttp {
            status: response.status,
        });
    }
    response.json_value()
}

fn load_cache(path: &Path) -> HashMap<String, HltbTimes> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    entries
        .into_iter()
        .filter_map(|(key, value)| {
            let value = value.as_object()?;
            Some((
                key,
                HltbTimes {
                    main: value_number(value.get("main")) as i64,
                    main_extra: value_number(value.get("mainExtra")) as i64,
                    completionist: value_number(value.get("completionist")) as i64,
                    ts: value_number(value.get("ts")).max(0.0) as u64,
                },
            ))
        })
        .collect()
}

fn save_cache(path: &Path, cache: &HashMap<String, HltbTimes>) {
    let Ok(json) = serde_json::to_string_pretty(cache) else {
        return;
    };
    let temporary = PathBuf::from(format!("{}.tmp", path.to_string_lossy()));
    if std::fs::write(&temporary, json).is_err() {
        return;
    }
    if std::fs::rename(&temporary, path).is_err() {
        // No Windows o rename não substitui um destino existente.
        let _ = std::fs::remove_file(path);
        let _ = std::fs::rename(&temporary, path);
    }
}

/// Cliente HLTB com cache de 30 dias. Sem `cache_path`, o cache fica só em
/// memória (útil em testes); com caminho, reproduz `hltb_cache.json`.
pub struct HltbClient {
    fetcher: Arc<dyn Fetcher>,
    clock: Arc<dyn Clock>,
    base: String,
    cache_path: Option<PathBuf>,
    memory: Mutex<HashMap<String, HltbTimes>>,
}

impl HltbClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            clock: Arc::new(SystemClock),
            base: HLTB_BASE.to_string(),
            cache_path: None,
            memory: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_path = Some(path.into());
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn cache_path(&self) -> Option<&Path> {
        self.cache_path.as_deref()
    }

    fn read_cache(&self) -> HashMap<String, HltbTimes> {
        match self.cache_path.as_deref() {
            Some(path) => load_cache(path),
            None => lock(&self.memory).clone(),
        }
    }

    fn write_cache(&self, cache: HashMap<String, HltbTimes>) {
        match self.cache_path.as_deref() {
            Some(path) => save_cache(path, &cache),
            None => *lock(&self.memory) = cache,
        }
    }

    /// `hltbBuscar`: `None` em qualquer falha, título vazio ou tempos zerados.
    pub async fn search(&self, title: &str) -> Option<HltbTimes> {
        let key = normalize_hltb_title(title);
        if key.is_empty() {
            return None;
        }
        let now = self.clock.now_ms();
        let mut cache = self.read_cache();
        if let Some(cached) = cache.get(&key) {
            if now.saturating_sub(cached.ts) < HLTB_CACHE_TTL_MS {
                return Some(HltbTimes {
                    main: cached.main,
                    main_extra: cached.main_extra,
                    completionist: cached.completionist,
                    ts: cached.ts,
                });
            }
        }

        let hash = match discover_hash_with_base(self.fetcher.as_ref(), &self.base).await {
            Ok(Some(hash)) => hash,
            Ok(None) => match SEEK_HASHES_FALLBACK.first() {
                Some(hash) => (*hash).to_string(),
                None => return None,
            },
            Err(_) => match SEEK_HASHES_FALLBACK.first() {
                Some(hash) => (*hash).to_string(),
                None => return None,
            },
        };
        let data = seek_with_base(self.fetcher.as_ref(), &self.base, &hash, title)
            .await
            .ok()?;
        let items = data.get("data").and_then(Value::as_array)?;
        if items.is_empty() {
            return None;
        }
        let selected = items
            .iter()
            .find(|item| normalize_hltb_title(&value_str(item.get("game_name"))) == key)
            .unwrap_or(&items[0]);
        let times = HltbTimes {
            main: crate::http::js_round(value_number(selected.get("comp_main")) / 60.0) as i64,
            main_extra: crate::http::js_round(value_number(selected.get("comp_plus")) / 60.0)
                as i64,
            completionist: crate::http::js_round(value_number(selected.get("comp_100")) / 60.0)
                as i64,
            // No JS o timestamp é lido depois do POST, no momento de montar o
            // resultado; assim o TTL começa no fim da busca.
            ts: self.clock.now_ms(),
        };
        if times.main == 0 && times.main_extra == 0 && times.completionist == 0 {
            return None;
        }
        cache.insert(key, times.clone());
        self.write_cache(cache);
        Some(times)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::ManualClock;
    use crate::http::Response;
    use crate::test_support::{json_response, MockFetcher};

    fn html_with_bundle() -> Response {
        Response::text(
            200,
            r#"<script src="/_next/static/chunks/pages/_app-abc123.js"></script>"#,
        )
    }

    fn seek_response() -> Value {
        json!({
            "data": [
                {"game_name": "Outro Jogo", "comp_main": 3600, "comp_plus": 7200, "comp_100": 9000},
                {"game_name": "Portal 2", "comp_main": 5400, "comp_plus": 7200, "comp_100": 10800}
            ]
        })
    }

    #[test]
    fn normalization_matches_the_client_rule() {
        assert_eq!(normalize_hltb_title("Portal 2"), "portal 2");
        assert_eq!(normalize_hltb_title("  Half-Life:  2!! "), "half life 2");
        assert_eq!(normalize_hltb_title("Ação"), "a o");
        assert_eq!(normalize_hltb_title("---"), "");
    }

    #[test]
    fn seek_body_is_exact() {
        assert_eq!(
            seek_body("Half-Life 2"),
            json!({
                "searchType": "games",
                "searchTerms": ["half", "life", "2"],
                "searchPage": 1,
                "size": 5,
                "searchOptions": {
                    "games": {
                        "userId": 0,
                        "platform": "",
                        "sortCategory": "popular",
                        "rangeCategory": "main",
                        "rangeTime": {"min": null, "max": null},
                        "gameplay": {"perspective": "", "flow": "", "genre": ""},
                        "modifier": ""
                    },
                    "users": {"sortCategory": "postcount"},
                    "filter": "",
                    "sort": 0,
                    "randomizer": 0
                }
            })
        );
    }

    #[tokio::test]
    async fn discovers_hash_from_plus_pattern() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#"const x="/api/seek/"+ "abc123def456";"#),
        ]);
        assert_eq!(
            discover_hash(&fetcher).await.unwrap().as_deref(),
            Some("abc123def456")
        );
    }

    #[tokio::test]
    async fn discovers_hash_from_concat_and_generic_patterns() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#"const x="/api/seek/".concat( "999abc");"#),
            html_with_bundle(),
            Response::text(200, "fetch('api/seek/abcdefabcdefabcdef12')"),
        ]);
        assert_eq!(
            discover_hash(&fetcher).await.unwrap().as_deref(),
            Some("999abc")
        );
        assert_eq!(
            discover_hash(&fetcher).await.unwrap().as_deref(),
            Some("abcdefabcdefabcdef12")
        );
    }

    #[tokio::test]
    async fn missing_bundle_or_hash_is_none_but_bundle_status_is_ignored() {
        let fetcher = MockFetcher::new(vec![Response::text(200, "<html>nada</html>")]);
        assert_eq!(discover_hash(&fetcher).await.unwrap(), None);
        assert_eq!(fetcher.requests().len(), 1);

        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(404, r#""/api/seek/" + "deadbeef""#),
        ]);
        assert_eq!(
            discover_hash(&fetcher).await.unwrap().as_deref(),
            Some("deadbeef")
        );
    }

    #[tokio::test]
    async fn seek_contract_uses_all_headers_and_timeout() {
        let fetcher = MockFetcher::new(vec![json_response(seek_response())]);
        let data = seek(&fetcher, "hash", "Portal 2").await.unwrap();
        assert_eq!(data["data"].as_array().unwrap().len(), 2);
        let request = &fetcher.requests()[0];
        assert_eq!(request.url, "https://howlongtobeat.com/api/seek/hash");
        assert_eq!(request.method, crate::http::Method::Post);
        assert_eq!(request.timeout, HLTB_TIMEOUT);
        assert!(request
            .headers
            .contains(&("Referer".into(), "https://howlongtobeat.com/".into())));
        assert!(request
            .headers
            .contains(&("Origin".into(), "https://howlongtobeat.com".into())));
        assert!(request
            .headers
            .contains(&("Content-Type".into(), "application/json".into())));
        let body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
        assert_eq!(body, seek_body("Portal 2"));
    }

    #[tokio::test]
    async fn full_lookup_selects_exact_match_and_rounds_minutes() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(seek_response()),
        ]);
        let clock = Arc::new(ManualClock::new(1_000));
        let client = HltbClient::new(fetcher.clone()).with_clock(clock);
        let times = client.search("Portal 2").await.unwrap();
        assert_eq!(
            times,
            HltbTimes {
                main: 90,
                main_extra: 120,
                completionist: 180,
                ts: 1_000,
            }
        );
        assert_eq!(fetcher.requests().len(), 3);
    }

    #[tokio::test]
    async fn full_lookup_falls_back_to_the_first_item() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(seek_response()),
        ]);
        let client = HltbClient::new(fetcher);
        let times = client.search("Jogo Desconhecido").await.unwrap();
        assert_eq!(times.main, 60);
        assert_eq!(times.main_extra, 120);
        assert_eq!(times.completionist, 150);
    }

    #[tokio::test]
    async fn cache_prevents_network_until_thirty_days_and_then_refetches() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(seek_response()),
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(seek_response()),
        ]);
        let clock = Arc::new(ManualClock::new(10_000));
        let client = HltbClient::new(fetcher.clone()).with_clock(clock.clone());
        let first = client.search("Portal 2").await.unwrap();
        assert_eq!(first.ts, 10_000);
        clock.advance(HLTB_CACHE_TTL_MS - 1);
        assert_eq!(client.search("Portal 2").await.unwrap(), first);
        assert_eq!(fetcher.requests().len(), 3);
        clock.advance(1);
        assert_eq!(
            client.search("Portal 2").await.unwrap().ts,
            10_000 + HLTB_CACHE_TTL_MS
        );
        assert_eq!(fetcher.requests().len(), 6);
    }

    #[tokio::test]
    async fn zero_times_are_not_cached_and_errors_are_silent() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(
                json!({"data": [{"game_name": "Portal 2", "comp_main": 0, "comp_plus": 0, "comp_100": 0}]}),
            ),
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(seek_response()),
        ]);
        let client = HltbClient::new(fetcher.clone());
        assert_eq!(client.search("Portal 2").await, None);
        // Sem cache: a segunda busca refaz o fluxo.
        assert_eq!(client.search("Portal 2").await.unwrap().main, 90);
        assert_eq!(fetcher.requests().len(), 6);

        let fetcher = MockFetcher::new(vec![Response::new(500, "")]);
        let client = HltbClient::new(fetcher);
        assert_eq!(client.search("Portal 2").await, None);
        assert_eq!(client.search("   ").await, None);
    }

    #[tokio::test]
    async fn seek_http_error_returns_none() {
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            Response::new(429, ""),
        ]);
        let client = HltbClient::new(fetcher);
        assert_eq!(client.search("Portal 2").await, None);
    }

    #[tokio::test]
    async fn cache_file_is_pretty_json_and_reused_by_a_new_client() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hltb_cache.json");
        let fetcher = MockFetcher::new(vec![
            html_with_bundle(),
            Response::text(200, r#""/api/seek/" + "abc""#),
            json_response(seek_response()),
        ]);
        let clock = Arc::new(ManualClock::new(50_000));
        let client = HltbClient::new(fetcher.clone())
            .with_cache_path(&path)
            .with_clock(clock.clone());
        client.search("Portal 2").await.unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"mainExtra\": 120"));
        assert!(text.contains("\"portal 2\""));

        let second_fetcher = MockFetcher::new(vec![]);
        let second = HltbClient::new(second_fetcher.clone())
            .with_cache_path(&path)
            .with_clock(clock);
        assert_eq!(second.search("Portal 2").await.unwrap().main, 90);
        assert!(second_fetcher.requests().is_empty());
        assert_eq!(second.cache_path(), Some(path.as_path()));
    }
}
