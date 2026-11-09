//! Contratos do registro de fontes: CRUD, cache/ETag, índice, busca 1:1,
//! getGame e payloads de sync. Persistência em memória/disco temporário.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcadia_catalog::http::Response;
use arcadia_catalog::sources::{
    buscar_no_indice, fold_titulo, match_titulo, next_watermarks, pull_sources, push_payload,
    source_id, valid_source_id, valid_source_url, CanonicalTitle, ChallengeSolver, FsSourceStorage,
    IndexEntry, ManualClock, MemorySourceStorage, SourceCatalog, SourceInfo, SourceRegistry,
    SourceStorage, INDEX_FILE, REGISTRY_FILE,
};
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use support::{header, json_response, ScriptedFetcher};

const URL: &str = "https://hydra.test/source.json";
const ID: &str = "784f996b323a";

fn manual_clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(1_700_000_000_000))
}

fn memory_registry(
    storage: Arc<dyn SourceStorage>,
    fetcher: Arc<ScriptedFetcher>,
    clock: Arc<ManualClock>,
) -> SourceRegistry {
    SourceRegistry::new(storage, fetcher).with_clock(clock)
}

fn cache_json(name: &str, downloads: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({ "name": name, "downloads": downloads })).unwrap()
}

fn write_registry(storage: &dyn SourceStorage, sources: &[SourceInfo]) {
    storage
        .write(REGISTRY_FILE, &serde_json::to_vec(sources).unwrap())
        .unwrap();
}

struct StaticCatalog {
    data: Mutex<std::collections::HashMap<String, Value>>,
    calls: AtomicUsize,
}

impl StaticCatalog {
    fn new(id: &str, data: Value) -> Arc<Self> {
        let mut map = std::collections::HashMap::new();
        map.insert(id.to_string(), data);
        Arc::new(Self {
            data: Mutex::new(map),
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SourceCatalog for StaticCatalog {
    async fn source_games(&self, source_id: &str) -> Option<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(source_id)
            .cloned()
    }
}

struct Challenge(Value);

#[async_trait]
impl ChallengeSolver for Challenge {
    async fn resolve(&self, _url: &str) -> Option<Value> {
        Some(self.0.clone())
    }
}

#[test]
fn ids_and_urls_match_the_frozen_helpers() {
    assert_eq!(source_id(URL), ID);
    assert_eq!(source_id("https://example.com/source.json"), "b2efa370e2de");
    assert!(valid_source_url("http://x/y"));
    assert!(valid_source_url("https://x/y"));
    assert!(!valid_source_url("ftp://x/y"));
    assert!(!valid_source_url("HTTP://x/y"));
    assert!(!valid_source_url(""));
    assert!(valid_source_id(ID));
    assert!(!valid_source_id("../x"));
    assert!(!valid_source_id(""));
}

#[tokio::test]
async fn add_source_downloads_validates_caches_and_persists_registry() {
    let storage = Arc::new(MemorySourceStorage::new());
    let clock = manual_clock();
    let fetcher = ScriptedFetcher::new(vec![json_response(json!({
        "name": "Fonte Teste",
        "downloads": [
            { "title": "Jogo A", "fileSize": "1 GB", "uris": ["magnet:?xt=urn:btih:a"] },
            { "title": "", "uris": ["magnet:?x"] },
            { "title": "Sem URI", "uris": [] }
        ]
    }))
    .header("etag", "W/\"1\"")
    .header("last-modified", "Mon, 09 Nov 2026 00:00:00 GMT")]);
    let registry = memory_registry(storage.clone(), fetcher.clone(), clock.clone());

    let result = registry.add_source(&format!("  {URL}  ")).await;
    assert!(result.ok);
    let source = result.source.unwrap();
    assert_eq!(source.id, ID);
    assert_eq!(source.url, URL);
    assert_eq!(source.name, "Fonte Teste");
    assert_eq!(source.etag, "W/\"1\"");
    assert_eq!(source.last_mod, "Mon, 09 Nov 2026 00:00:00 GMT");
    assert_eq!(source.added_at, 1_700_000_000_000);
    assert_eq!(source.count, Some(1));

    assert!(storage.contains(&format!("sources/{ID}.json")));
    let cache: Value =
        serde_json::from_slice(&storage.get(&format!("sources/{ID}.json")).unwrap()).unwrap();
    assert_eq!(cache["name"], "Fonte Teste");
    assert_eq!(cache["downloads"].as_array().unwrap().len(), 1);
    assert_eq!(cache["downloads"][0]["title"], "Jogo A");

    let saved: Value = serde_json::from_slice(&storage.get(REGISTRY_FILE).unwrap()).unwrap();
    assert_eq!(saved[0]["id"], ID);
    assert_eq!(saved[0]["name"], "Fonte Teste");
    assert_eq!(saved[0]["count"], 1);

    let duplicate = registry.add_source(URL).await;
    assert!(!duplicate.ok);
    assert_eq!(duplicate.error.as_deref(), Some("fonte já adicionada"));
    assert_eq!(fetcher.request_count(), 1);

    let invalid = registry.add_source("ftp://hydra.test/source.json").await;
    assert_eq!(invalid.error.as_deref(), Some("URL inválida"));
    assert_eq!(fetcher.request_count(), 1);

    let results = registry.search("jogo a", 40).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].reference, format!("{ID}:0"));
    assert_eq!(results[0].title, "Jogo A");
    assert_eq!(results[0].file_size, "1 GB");
    assert_eq!(results[0].src, "Fonte Teste");
}

#[tokio::test]
async fn add_source_rejects_empty_malformed_and_invalid_json_catalogs() {
    for (response, expected) in [
        (
            json_response(json!({ "downloads": [] })),
            "formato inválido (esperado { downloads: [...] })",
        ),
        (
            json_response(json!({ "foo": 1 })),
            "formato inválido (esperado { downloads: [...] })",
        ),
    ] {
        let storage = Arc::new(MemorySourceStorage::new());
        let registry = memory_registry(
            storage.clone(),
            ScriptedFetcher::new(vec![response]),
            manual_clock(),
        );
        let result = registry.add_source(URL).await;
        assert!(!result.ok);
        assert_eq!(result.error.as_deref(), Some(expected));
        assert!(!storage.contains(REGISTRY_FILE));
    }

    let storage = Arc::new(MemorySourceStorage::new());
    let registry = memory_registry(
        storage.clone(),
        ScriptedFetcher::new(vec![Response::text(200, "<html>")]),
        manual_clock(),
    );
    let result = registry.add_source(URL).await;
    assert!(!result.ok);
    assert!(result.error.unwrap().contains("resposta JSON inválida"));
    assert!(!storage.contains(REGISTRY_FILE));
}

#[tokio::test]
async fn sync_sources_sends_conditional_headers_and_304_keeps_count() {
    let storage = Arc::new(MemorySourceStorage::new());
    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({
            "name": "Fonte",
            "downloads": [
                { "title": "A", "uris": ["magnet:a"] },
                { "title": "B", "uris": ["magnet:b"] }
            ]
        }))
        .header("etag", "W/\"v1\"")
        .header("last-modified", "Mon"),
        Response::new(304, ""),
    ]);
    let registry = memory_registry(storage.clone(), fetcher.clone(), manual_clock());
    let added = registry.add_source(URL).await;
    assert!(added.ok);

    let synced = registry.sync_sources().await;
    assert!(synced.ok);
    assert_eq!(synced.results.len(), 1);
    assert_eq!(synced.results[0].id, ID);
    assert!(synced.results[0].ok);
    assert_eq!(synced.results[0].mudou, Some(false));
    assert_eq!(synced.results[0].count, Some(2));
    assert_eq!(synced.results[0].error, None);

    let requests = fetcher.requests();
    assert_eq!(header(&requests[1], "if-none-match"), Some("W/\"v1\""));
    assert_eq!(header(&requests[1], "if-modified-since"), Some("Mon"));
    assert_eq!(header(&requests[1], "user-agent"), Some("arcadia"));
    assert_eq!(requests[1].timeout, Duration::from_secs(60));

    let saved: Value = serde_json::from_slice(&storage.get(REGISTRY_FILE).unwrap()).unwrap();
    assert_eq!(saved[0]["count"], 2);
    assert_eq!(saved[0]["etag"], "W/\"v1\"");
}

#[tokio::test]
async fn sync_sources_reports_errors_per_source_and_updates_successful_etags() {
    let storage = Arc::new(MemorySourceStorage::new());
    let sources = vec![
        SourceInfo::new("aaaaaaaaaaaa", "https://hydra.test/a.json", 1),
        SourceInfo::new("bbbbbbbbbbbb", "https://hydra.test/b.json", 2),
    ];
    write_registry(storage.as_ref(), &sources);

    let fetcher = ScriptedFetcher::new(vec![
        json_response(json!({
            "name": "A novo",
            "downloads": [{ "title": "A1", "uris": ["magnet:a"] }]
        }))
        .header("etag", "\"a2\""),
        Response::text(500, "boom"),
    ]);
    let registry = memory_registry(storage.clone(), fetcher, manual_clock());
    let synced = registry.sync_sources().await;
    assert!(synced.ok);
    assert_eq!(synced.results.len(), 2);
    assert!(synced.results[0].ok);
    assert_eq!(synced.results[0].mudou, Some(true));
    assert_eq!(synced.results[0].count, Some(1));
    assert!(!synced.results[1].ok);
    assert_eq!(synced.results[1].error.as_deref(), Some("HTTP 500"));

    let saved: Value = serde_json::from_slice(&storage.get(REGISTRY_FILE).unwrap()).unwrap();
    assert_eq!(saved[0]["etag"], "\"a2\"");
    assert_eq!(saved[0]["name"], "A novo");
    assert_eq!(saved[1]["etag"], "");
}

#[tokio::test]
async fn remove_source_drops_registry_cache_and_index() {
    let storage = Arc::new(MemorySourceStorage::new());
    let fetcher = ScriptedFetcher::new(vec![json_response(json!({
        "name": "Fonte",
        "downloads": [{ "title": "Jogo", "uris": ["magnet:x"] }]
    }))]);
    let registry = memory_registry(storage.clone(), fetcher, manual_clock());
    assert!(registry.add_source(URL).await.ok);
    assert_eq!(registry.search("jogo", 40).await.len(), 1);
    assert!(storage.contains(INDEX_FILE));

    let removed = registry.remove_source(ID);
    assert!(removed.ok);
    let saved: Value = serde_json::from_slice(&storage.get(REGISTRY_FILE).unwrap()).unwrap();
    assert_eq!(saved, json!([]));
    assert!(!storage.contains(&format!("sources/{ID}.json")));
    assert!(!storage.contains(INDEX_FILE));
    assert_eq!(registry.list().len(), 0);

    let missing = registry.remove_source(ID);
    assert!(!missing.ok);
    assert_eq!(missing.error.as_deref(), Some("fonte não encontrada"));
}

#[tokio::test]
async fn search_uses_remote_catalog_first_then_local_cache_and_persists_index() {
    let storage = Arc::new(MemorySourceStorage::new());
    write_registry(storage.as_ref(), &[SourceInfo::new(ID, URL, 10)]);
    storage.put(
        &format!("sources/{ID}.json"),
        cache_json(
            "Local",
            json!([{ "title": "Jogo Local", "fileSize": "2 GB", "uris": ["magnet:l"] }]),
        ),
    );
    let catalog = StaticCatalog::new(
        ID,
        json!({
            "name": "Remoto",
            "downloads": [{ "title": "Jogo Remoto", "fileSize": "3 GB", "uploadDate": "hoje", "uris": ["magnet:r"] }]
        }),
    );
    let registry = memory_registry(
        storage.clone(),
        ScriptedFetcher::new(vec![]),
        manual_clock(),
    )
    .with_catalog(catalog.clone());

    let remote = registry.search("remoto", 40).await;
    assert_eq!(remote.len(), 1);
    assert_eq!(remote[0].reference, format!("{ID}:0"));
    assert_eq!(remote[0].file_size, "3 GB");
    assert_eq!(remote[0].upload_date, "hoje");
    assert_eq!(remote[0].src, "Remoto");
    assert!(registry.search("local", 40).await.is_empty());
    assert_eq!(catalog.calls(), 1);

    let index_json: Value = serde_json::from_slice(&storage.get(INDEX_FILE).unwrap()).unwrap();
    assert_eq!(index_json[0]["title"], "Jogo Remoto");
    assert_eq!(index_json[0]["fold"], "jogo remoto");
    assert_eq!(index_json[0]["fold"], fold_titulo("Jogo Remoto"));

    // Sem catálogo remoto e sem índice em memória, o cache local responde.
    storage.remove(INDEX_FILE);
    let local = memory_registry(
        storage.clone(),
        ScriptedFetcher::new(vec![]),
        manual_clock(),
    );
    let results = local.search("local", 40).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Jogo Local");
    assert_eq!(results[0].src, "Local");
}

#[tokio::test]
async fn search_loads_persisted_index_without_rebuilding() {
    let storage = Arc::new(MemorySourceStorage::new());
    let entry = json!([{
        "ref": "abc:0",
        "title": "Portal 2",
        "fold": "portal 2",
        "fileSize": "8 GB",
        "uploadDate": "",
        "src": "Fonte"
    }]);
    storage.put(INDEX_FILE, serde_json::to_vec(&entry).unwrap());
    let fetcher = ScriptedFetcher::new(vec![]);
    let registry = memory_registry(storage.clone(), fetcher.clone(), manual_clock());

    let results = registry.search("portal 2", 40).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].reference, "abc:0");
    assert_eq!(fetcher.request_count(), 0);
}

#[tokio::test]
async fn get_game_returns_full_payload_with_source_name_and_caches_last_file() {
    let storage = Arc::new(MemorySourceStorage::new());
    write_registry(storage.as_ref(), &[SourceInfo::new(ID, URL, 10)]);
    storage.put(
        &format!("sources/{ID}.json"),
        cache_json(
            "Fonte A",
            json!([
                { "title": "Jogo 0", "uris": ["magnet:0"], "fileSize": "1 GB" },
                { "title": "Jogo 1", "uris": ["magnet:1"], "fileSize": "2 GB" }
            ]),
        ),
    );
    let registry = memory_registry(
        storage.clone(),
        ScriptedFetcher::new(vec![]),
        manual_clock(),
    );

    let result = registry.get_game(&format!("{ID}:1")).await;
    assert!(result.ok);
    assert_eq!(result.source.as_deref(), Some("Fonte A"));
    assert_eq!(result.game.as_ref().unwrap()["uris"][0], "magnet:1");
    assert!(result.game.unwrap().get("fold").is_none());

    let first = registry.get_game(&format!("{ID}:")).await;
    assert!(first.ok);
    assert_eq!(first.game.unwrap()["title"], "Jogo 0");
    assert_eq!(
        registry.get_game(&format!("{ID}:9")).await.error.as_deref(),
        Some("jogo não encontrado")
    );
    assert_eq!(
        registry.get_game("missing:0").await.error.as_deref(),
        Some("fonte nao encontrada")
    );
}

#[tokio::test]
async fn get_game_reports_unavailable_catalog_and_uses_remote_payload() {
    let storage = Arc::new(MemorySourceStorage::new());
    write_registry(storage.as_ref(), &[SourceInfo::new(ID, URL, 10)]);
    let registry = memory_registry(
        storage.clone(),
        ScriptedFetcher::new(vec![]),
        manual_clock(),
    );
    assert_eq!(
        registry.get_game(&format!("{ID}:0")).await.error.as_deref(),
        Some("catalogo da fonte indisponivel")
    );

    let catalog = StaticCatalog::new(
        ID,
        json!({
            "name": "Remoto",
            "downloads": [{ "title": "Jogo Remoto", "uris": ["magnet:r"] }]
        }),
    );
    let registry = memory_registry(storage, ScriptedFetcher::new(vec![]), manual_clock())
        .with_catalog(catalog.clone());
    let first = registry.get_game(&format!("{ID}:0")).await;
    assert!(first.ok);
    assert_eq!(first.source.as_deref(), Some("Remoto"));
    let second = registry.get_game(&format!("{ID}:0")).await;
    assert!(second.ok);
    assert_eq!(catalog.calls(), 1);
}

#[tokio::test]
async fn push_and_pull_shapes_match_supabase_sources() {
    let mut existing = SourceInfo::new("aaaaaaaaaaaa", "https://a/source.json", 1);
    existing.name = "A".into();
    existing.etag = "W/\"1\"".into();
    existing.last_mod = "Mon".into();
    existing.count = Some(9);
    let mut extra = Map::new();
    extra.insert("apiKey".into(), json!("não sincroniza"));
    existing.extra = extra;
    let registry = vec![existing.clone()];

    let mut watermarks = Map::new();
    watermarks.insert(
        "old-source-1".into(),
        json!({ "url": "https://old", "name": "Old" }),
    );

    let payload = push_payload(&registry, &watermarks);
    assert_eq!(
        payload,
        vec![
            json!({ "source_id": "aaaaaaaaaaaa", "url": "https://a/source.json", "name": "A" }),
            json!({ "source_id": "old-source-1", "removed": true })
        ]
    );

    let marks = next_watermarks(&registry);
    assert_eq!(
        marks.get("aaaaaaaaaaaa"),
        Some(&json!({ "url": "https://a/source.json", "name": "A" }))
    );
    assert_eq!(marks.len(), 1);

    let mut local = registry.clone();
    local.push(SourceInfo::new(
        "bbbbbbbbbbbb",
        "https://local/source.json",
        2,
    ));
    let rows = vec![
        json!({ "source_id": "aaaaaaaaaaaa", "url": "https://changed", "name": "Changed" }),
        json!({ "source_id": "cccccccccccc", "url": "https://c/source.json", "name": "C" }),
        json!({ "source_id": "cccccccccccc", "url": "https://dup", "name": "Dup" }),
        json!({ "url": "https://missing-id" }),
    ];
    let outcome = pull_sources(&mut local, &rows, 99);
    assert_eq!(outcome.pulled, 4);
    assert_eq!(outcome.added, 1);
    assert!(outcome.changed);
    assert_eq!(local.len(), 3);
    assert_eq!(local[0].url, "https://a/source.json");
    let pulled = local.last().unwrap();
    assert_eq!(pulled.id, "cccccccccccc");
    assert_eq!(pulled.url, "https://c/source.json");
    assert_eq!(pulled.name, "C");
    assert_eq!(pulled.etag, "");
    assert_eq!(pulled.last_mod, "");
    assert_eq!(pulled.added_at, 99);
    assert_eq!(pulled.count, Some(0));
}

#[tokio::test]
async fn registry_sync_helpers_write_pull_and_expose_push_payload() {
    let storage = Arc::new(MemorySourceStorage::new());
    let clock = manual_clock();
    write_registry(storage.as_ref(), &[SourceInfo::new(ID, URL, 1)]);
    let registry = memory_registry(storage.clone(), ScriptedFetcher::new(vec![]), clock.clone());

    let payload = registry.push_payload(&Map::new());
    assert_eq!(
        payload,
        vec![json!({ "source_id": ID, "url": URL, "name": "" })]
    );
    assert_eq!(registry.next_watermarks().len(), 1);

    let outcome = registry.apply_pull(&[json!({
        "source_id": "cccccccccccc",
        "url": "https://c/source.json",
        "name": "C"
    })]);
    assert_eq!(outcome.added, 1);
    assert_eq!(registry.list().len(), 2);
    assert_eq!(registry.list()[1].added_at, 1_700_000_000_000);
}

#[tokio::test]
async fn cloudflare_403_uses_the_injected_challenge_solver() {
    let data = json!({
        "name": "Via navegador",
        "downloads": [{ "title": "Jogo", "uris": ["magnet:x"] }]
    });
    let storage = Arc::new(MemorySourceStorage::new());
    let registry = memory_registry(
        storage,
        ScriptedFetcher::new(vec![Response::new(403, "<html>challenge</html>")
            .header("server", "cloudflare")
            .header("cf-mitigated", "challenge")]),
        manual_clock(),
    )
    .with_challenge(Arc::new(Challenge(data)));
    let result = registry.add_source(URL).await;
    assert!(result.ok);
    assert_eq!(result.source.unwrap().count, Some(1));
}

#[tokio::test]
async fn cloudflare_403_without_solver_reports_http_403() {
    let storage = Arc::new(MemorySourceStorage::new());
    let registry = memory_registry(
        storage.clone(),
        ScriptedFetcher::new(vec![
            Response::new(403, "<html>").header("server", "cloudflare")
        ]),
        manual_clock(),
    );
    let result = registry.add_source(URL).await;
    assert!(!result.ok);
    assert_eq!(result.error.as_deref(), Some("HTTP 403"));
    assert!(!storage.contains(REGISTRY_FILE));
}

#[tokio::test]
async fn fs_storage_splits_account_registry_from_global_cache() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let account_dir = data_dir.join("contas").join("zes");
    let storage = Arc::new(FsSourceStorage::with_registry(&data_dir, &account_dir));
    let fetcher = ScriptedFetcher::new(vec![json_response(json!({
        "name": "Fonte",
        "downloads": [{ "title": "Jogo", "uris": ["magnet:x"] }]
    }))]);
    let registry = memory_registry(storage.clone(), fetcher, manual_clock());
    assert!(registry.add_source(URL).await.ok);

    assert!(account_dir.join(REGISTRY_FILE).exists());
    assert!(data_dir.join("sources").join(format!("{ID}.json")).exists());
    assert!(!account_dir.join("sources").exists());

    let reread = SourceRegistry::new(storage, Arc::new(ScriptedFetcher::new(vec![])))
        .with_clock(manual_clock());
    assert_eq!(reread.list().len(), 1);
    assert_eq!(reread.list()[0].id, ID);
}

// ── port fiel de app/test/store-match.test.js ──────────────────────────────

fn casa(alvo: &str, candidato: &str) -> bool {
    match_titulo(alvo, candidato).is_some()
}

fn rank(alvo: &str, candidatos: &[&str]) -> Vec<String> {
    let mut matches: Vec<(&str, u8, usize)> = candidatos
        .iter()
        .filter_map(|candidate| {
            match_titulo(alvo, candidate).map(|matched| (*candidate, matched.score, matched.extra))
        })
        .collect();
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
    matches.into_iter().map(|item| item.0.to_string()).collect()
}

#[test]
fn word_boundary_short_tokens_do_not_match_inside_words() {
    assert!(!casa(
        "ARK: Survival Evolved",
        "Boris and the Dark Survival Free Download (V1.13)"
    ));
    assert!(!casa(
        "ARK: Survival Evolved",
        "Shark Siege: Together Survival [DODI Repack]"
    ));
    assert!(!casa(
        "Raft",
        "Warcraft II Battlenet Edition v2.02-v5 [GOG]"
    ));
    assert!(!casa(
        "Bone: Out From Boneville",
        "Albedo: Eyes from Outer Space [GOG]"
    ));
    assert!(casa(
        "ARK: Survival Evolved",
        "ARK: Survival Evolved Free Download"
    ));
}

#[test]
fn sequence_numbers_must_match() {
    assert!(!casa("Far Cry 3", "Far Cry 2: Fortune's Edition [GOG]"));
    assert!(casa("Far Cry 3", "Far Cry 3 Free Download [SteamGG]"));
    assert!(!casa(
        "Baldur's Gate 3",
        "Baldur's Gate: Enhanced Edition v2.6.6.0-p [GOG]"
    ));
    assert!(!casa(
        "Baldur's Gate 3",
        "Baldur's Gate 2: Enhanced Edition v2.6.6.0 [GOG]"
    ));
    assert!(casa("Baldur's Gate 3", "Baldur's Gate 3 Free Download"));
    assert!(!casa(
        "Sid Meier's Civilization V",
        "Sid Meiers Civilization VII Free Download"
    ));
}

#[test]
fn target_without_number_does_not_match_numbered_sequel() {
    assert!(!casa(
        "Portal",
        "Portal 2 Free Download [Build-23973718+Co-Op] [SteamGG]"
    ));
    assert!(casa("Portal", "Portal"));
    assert!(!casa(
        "Half-Life",
        "Half Life 2 (v16.11.2024) [Pre-Instalado] [Kazumi]"
    ));
    assert!(casa("Half-Life", "Half-Life"));
    assert!(casa(
        "Half-Life 2",
        "Half-Life 2 (v16.11.2024) [Pre-Instalado]"
    ));
    assert!(!casa(
        "Left 4 Dead",
        "Left 4 Dead 2 Free Download [Build-23990068+Online]"
    ));
    assert!(!casa("Left 4 Dead", "Left 4 Dead 2 Build 04102021"));
}

#[test]
fn dlc_requires_the_suffix() {
    let dlc = "Assassin's Creed Black Flag Resynced - Dragon Storm Character Pack";
    assert!(!casa(
        dlc,
        "Assassins Creed Black Flag Resynced Free Download [v1.0.6+Bonus] [SteamGG]"
    ));
    assert!(casa(
        dlc,
        "Assassin's Creed Black Flag Resynced - Dragon Storm Character Pack Free Download"
    ));
    assert!(!casa(
        "Half-Life 2: Episode One",
        "Half-Life 2: Episode Two Free Download"
    ));
    assert!(!casa(
        "Half-Life 2: Episode One",
        "Fears to Fathom Episode 1 Home Alone Free Download"
    ));
    assert!(casa(
        "Half-Life 2: Episode One",
        "Half-Life 2: Episode One Free Download"
    ));
}

#[test]
fn versions_are_not_sequences() {
    assert!(casa("Brawlhalla", "Brawlhalla v3.20 [Dauphong]"));
    assert!(!casa("Hitman 2", "Hitman Blood Money Free Download (v1.2)"));
    assert!(casa(
        "Mount & Blade II: Bannerlord",
        "Mount and Blade II Bannerlord v1 1 6-FLT"
    ));
}

#[test]
fn standalone_i_is_a_pronoun_not_a_roman_numeral() {
    assert!(casa("Bread", "I am Bread Free Download"));
    assert!(casa(
        "The Last of Us Part I",
        "The Last of Us Part I Free Download"
    ));
    assert!(!casa(
        "The Last of Us Part I",
        "The Last of Us Part II Remastered"
    ));
    assert!(!casa("Mega Man X", "Mega Man X4"));
}

#[test]
fn metadata_numbers_are_not_sequences() {
    assert!(casa(
        "The Last of Us Part I",
        "The Last of Us: Part I - Digital Deluxe Edition (v1.0.4.1 + All DLCs + Bonus Content + Crash/Shaders Fixes + MULTi24) (From 40 GB) - [DODI Repack]"
    ));
    assert!(casa(
        "Black Myth: Wukong",
        "Black Myth: Wukong: Digital Deluxe Edition (v1.0.6 + All DLCs + Bonus Content + MULTi12) (From 95 GB) [DODI Repack]"
    ));
    assert!(casa(
        "Cyberpunk 2077",
        "Cyberpunk 2077 (v2.21 + All DLCs) (From 46 GB) - [DODI Repack]"
    ));
    assert!(casa(
        "Cities: Skylines",
        "Cities: Skylines - Collection (v1.18.1-f3 + All DLCs) (From 6 GB)"
    ));
    assert!(casa(
        "7 Days to Die",
        "7 Days to Die (v1.0 b333) + 9 DLCs [DODI Repack]"
    ));
    assert!(casa(
        "7 Days to Die",
        "7 Days to Die Alpha 16 Free Download"
    ));
    assert!(casa("7 Days to Die", "7 Days to Die 64 Bit Free Download"));
    assert!(!casa("Portal", "Portal 2 (From 5 GB)"));
    assert!(!casa(
        "Far Cry 3",
        "Far Cry 2: Fortune's Edition (From 40 GB)"
    ));
}

#[test]
fn year_after_roman_numeral_is_not_a_version() {
    assert!(casa(
        "Battlefield V",
        "Battlefield V (2018) PC | Repack ot xatab"
    ));
    assert!(casa("Battlefield V", "Battlefield V Free Download"));
    assert!(!casa(
        "Battlefield V",
        "Battlefield 1 (2016) PC | Repack ot xatab"
    ));
    assert!(casa(
        "Grand Theft Auto V",
        "Grand Theft Auto V (2013) [Xatab]"
    ));
}

#[test]
fn accents_are_folded_not_removed() {
    assert!(casa(
        "God of War Ragnarök",
        "God of War Ragnarok Free Download [SteamGG]"
    ));
    assert!(casa(
        "God of War Ragnarok",
        "God of War Ragnarök Free Download"
    ));
    assert!(!casa("God of War Ragnarök", "God of War [GOG]"));
}

#[test]
fn ranking_is_exact_then_prefix_then_phrase_then_words() {
    let candidates = [
        "Far Cry 2: Fortune's Edition [GOG]",
        "Far Cry 3: Blood Dragon Free Download",
        "Far Cry 3 Free Download [SteamGG]",
        "Far Cry 3",
    ];
    let sorted = rank("Far Cry 3", &candidates);
    assert_eq!(sorted[0], "Far Cry 3");
    assert_eq!(sorted[1], "Far Cry 3 Free Download [SteamGG]");
    assert!(!sorted.contains(&"Far Cry 2: Fortune's Edition [GOG]".to_string()));
}

fn index_entry(reference: &str, title: &str) -> IndexEntry {
    IndexEntry {
        reference: reference.into(),
        title: title.into(),
        fold: fold_titulo(title),
        ..IndexEntry::default()
    }
}

#[test]
fn search_reranks_before_the_limit_cutoff() {
    let mut index = Vec::new();
    for position in 0..60 {
        index.push(index_entry(
            &format!("x:{position}"),
            &format!("Far Cry 2 Fortune's Edition {position}"),
        ));
    }
    index.push(index_entry("x:ok", "Far Cry 3 Free Download [SteamGG]"));
    let results = buscar_no_indice(&index, "Far Cry 3", 50);
    assert_eq!(results[0].reference, "x:ok");
    assert!(results.len() <= 50);
}

#[test]
fn search_keeps_the_partial_fallback_of_the_sources_tab() {
    let index = vec![
        index_entry("a:0", "Far Cry 3 Free Download [SteamGG]"),
        index_entry("a:1", "Farcry Collection [GOG]"),
    ];
    let results = buscar_no_indice(&index, "farc", 40);
    assert!(results.iter().any(|game| game.reference == "a:1"));
    assert!(results.iter().all(|game| game.extra.is_empty()));
}

#[test]
fn canonical_title_exposes_the_internal_shape() {
    let canonical: CanonicalTitle = arcadia_catalog::sources::titulo_canonico("Far Cry 3");
    assert_eq!(canonical.palavras, vec!["far", "cry", "3"]);
    assert!(canonical.seq.contains(&3));
    assert_eq!(canonical.exigidas, vec!["far", "cry", "#3"]);
    assert_eq!(canonical.compacto, "farcry3");
}
