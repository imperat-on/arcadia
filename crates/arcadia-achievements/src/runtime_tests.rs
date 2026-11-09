//! Testes do runtime vivo. Ficam num módulo separado só pelo tamanho; usam
//! diretórios temporários reais e fixtures KeyValues sintéticas (nunca timers,
//! watchers do SO ou rede).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::cracked_watcher::CrackEnvironment;
use crate::events::{UnlockEvent, TOAST_WINDOW_SECONDS};
use crate::fsx::{Clock, Platform, SystemFs, WatcherFs};
use crate::manager::AchievementRecord;
use crate::runtime::{AchievementsRuntime, CaptureGate};
use crate::schema::schema_path;
use crate::test_support::{progress_bin, schema_bin, schema_bin_hidden, SchemaItem};

struct Fixture {
    root: PathBuf,
    data: PathBuf,
    steam: PathBuf,
    stats: PathBuf,
    appdata: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "arcadia-runtime-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let data = root.join("dados");
        let steam = root.join("steam");
        let stats = steam.join("appcache").join("stats");
        let appdata = root.join("roaming");
        for dir in [&data, &stats, &appdata] {
            fs::create_dir_all(dir).unwrap();
        }
        Self {
            root,
            data,
            steam,
            stats,
            appdata,
        }
    }

    fn runtime(&self, account: &str, appids: &[&str]) -> AchievementsRuntime {
        AchievementsRuntime::system(&self.data, account, &self.steam, appids.iter().copied())
    }

    fn account_dir(&self, account: &str) -> PathBuf {
        if account.is_empty() {
            self.data.clone()
        } else {
            self.data.join("contas").join(account)
        }
    }

    fn write_store(&self, account: &str, value: Value) {
        let dir = self.account_dir(account);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("achievements.json"), value.to_string()).unwrap();
    }

    fn read_store(&self, account: &str) -> Value {
        let text = fs::read_to_string(self.account_dir(account).join("achievements.json"))
            .unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&text).unwrap()
    }

    fn write_schema(&self, appid: &str, items: &[SchemaItem<'_>]) {
        fs::write(schema_path(&self.stats, appid), schema_bin(items)).unwrap();
    }

    fn write_schema_hidden(&self, appid: &str, items: &[SchemaItem<'_>], hidden: &[(&str, &str)]) {
        fs::write(
            schema_path(&self.stats, appid),
            schema_bin_hidden(items, hidden),
        )
        .unwrap();
    }

    fn write_progress(&self, appid: &str, entries: &[(&str, &str, i64)]) {
        fs::write(
            self.stats
                .join(format!("UserGameStats_76561198000000000_{appid}.bin")),
            progress_bin(entries),
        )
        .unwrap();
    }

    fn write_progress_bytes(&self, appid: &str, bytes: &[u8]) {
        fs::write(
            self.stats
                .join(format!("UserGameStats_76561198000000000_{appid}.bin")),
            bytes,
        )
        .unwrap();
    }

    fn crack_environment(&self) -> CrackEnvironment {
        CrackEnvironment {
            home: self.root.clone(),
            appdata: Some(self.appdata.clone()),
            local_app_data: Some(self.root.join("local")),
            public: Some(self.root.join("public")),
            program_data: Some(self.root.join("programdata")),
            documents: Some(self.root.join("documents")),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn item(apiname: &str, title: &str, block: i64, bit: i64, achieved: bool) -> Value {
    json!({
        "apiname": apiname,
        "title": title,
        "desc": format!("desc {apiname}"),
        "icon": format!("{apiname}.jpg"),
        "block": block,
        "bit": bit,
        "achieved": achieved,
        "unlock": if achieved { 100 } else { 0 },
        "percent": 12.5,
    })
}

fn local_item(apiname: &str, title: &str, block: i64, bit: i64) -> Value {
    let mut value = item(apiname, title, block, bit, true);
    value
        .as_object_mut()
        .unwrap()
        .insert("localUnlock".into(), Value::Bool(true));
    value
}

#[test]
fn first_tick_is_a_silent_snapshot_then_new_keys_unlock_once() {
    let fixture = Fixture::new("first-tick");
    fixture.write_store(
        "tester",
        json!({"42": {"items": [
            item("ACH01", "Primeiros passos", 1, 0, false),
            item("ACH02", "Segunda", 1, 1, false),
        ]}}),
    );
    fixture.write_progress("42", &[("1", "0", 100)]);
    let mut runtime = fixture.runtime("tester", &["42"]);

    let first = runtime.tick();
    assert!(!first.skipped);
    assert!(first.events.is_empty(), "snapshot inicial é silencioso");
    assert_eq!(first.scanned_files, 1);
    assert_eq!(first.scanned_appids, 1);
    assert!(!first.changed);
    assert!(runtime.tick().events.is_empty(), "reobservar é no-op");

    fixture.write_progress("42", &[("1", "0", 100), ("1", "1", 200)]);
    let report = runtime.tick();
    assert_eq!(report.events.len(), 1);
    assert!(report.changed);
    let event = report.unlocks().next().unwrap();
    assert_eq!(event.appid, "42");
    assert_eq!(event.key, "1|1");
    assert_eq!(event.apiname, "ACH02");
    assert_eq!(event.title, "Segunda");
    assert_eq!(event.desc, "desc ACH02");
    assert_eq!(event.provider, "steam");
    assert_eq!(event.percent, 12.5);
    assert_eq!(event.unlock, 200);
    assert!(event.local_unlock);
    assert_eq!((event.done, event.total), (1, 2));
    assert!(event.is_recent(400, TOAST_WINDOW_SECONDS));
    assert_eq!(report.unindexed.len(), 0);

    let stored = fixture.read_store("tester");
    let items = stored["42"]["items"].as_array().unwrap();
    assert_eq!(items[1]["achieved"], json!(true));
    assert_eq!(items[1]["unlock"], json!(200));
    assert_eq!(items[1]["localUnlock"], json!(true));
    assert_eq!(
        items[0]["achieved"],
        json!(false),
        "baseline não marca o antigo"
    );

    let queued = runtime.queue().get("42", "ACH02").unwrap();
    assert_eq!(queued.unlocked_at, 200);
    assert!(queued.achieved);
    assert_eq!(queued.title.as_deref(), Some("Segunda"));
    assert_eq!(runtime.queue().len(), 1);
    assert!(runtime.tick().events.is_empty(), "dedupe entre polls");
}

#[test]
fn catch_up_rebuilds_schema_and_applies_progress_silently() {
    let fixture = Fixture::new("catch-up");
    fixture.write_schema(
        "42",
        &[
            ("1", "0", "ACH01", &[("brazilian", "Primeiros passos")]),
            ("1", "1", "ACH02", &[("brazilian", "Segunda")]),
        ],
    );
    fixture.write_progress("42", &[("1", "0", 500)]);
    let mut runtime = fixture.runtime("tester", &["42"]);

    let report = runtime.catch_up();
    assert_eq!(report.updated, 1);
    assert_eq!(report.total_appids, 1);
    assert_eq!(report.total, 1);
    assert!(!report.skipped_progress);
    assert_eq!(report.sync_queued, 2);
    assert!(report.errors.is_empty());

    let stored = fixture.read_store("tester");
    let items = stored["42"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["apiname"], json!("ACH01"));
    assert_eq!(items[0]["title"], json!("Primeiros passos"));
    assert_eq!(items[0]["achieved"], json!(true));
    assert_eq!(items[0]["unlock"], json!(500));
    assert_eq!(items[1]["achieved"], json!(false));
    assert!(
        runtime.tick().events.is_empty(),
        "catch_up prepara o snapshot"
    );

    fixture.write_progress("42", &[("1", "0", 500), ("1", "1", 600)]);
    let tick = runtime.tick();
    assert_eq!(tick.events.len(), 1);
    let event = tick.unlocks().next().unwrap();
    assert_eq!(event.apiname, "ACH02");
    assert_eq!(event.unlock, 600);
    assert_eq!((event.done, event.total), (2, 2));
}

#[test]
fn capture_gate_skips_tick_and_forced_one_shot_releases_exactly_one_pass() {
    let fixture = Fixture::new("gate");
    fixture.write_store(
        "tester",
        json!({"42": {"items": [
            item("ACH01", "Primeiros passos", 1, 0, false),
            item("ACH02", "Segunda", 1, 1, false),
        ]}}),
    );
    fixture.write_progress("42", &[("1", "0", 100)]);
    let mut runtime = fixture.runtime("tester", &["42"]);
    assert!(runtime.tick().events.is_empty());

    runtime.set_capture_gate(CaptureGate::blocked());
    fixture.write_progress("42", &[("1", "0", 100), ("1", "1", 200)]);
    let skipped = runtime.tick();
    assert!(skipped.skipped);
    assert!(skipped.events.is_empty());
    assert!(!skipped.changed);
    assert_eq!(
        fixture.read_store("tester")["42"]["items"][1]["achieved"],
        json!(false),
        "gate bloqueado não ingere"
    );

    runtime.force_next_capture();
    let forced = runtime.tick();
    assert!(forced.forced);
    assert!(!forced.skipped);
    assert_eq!(forced.events.len(), 1);
    assert_eq!(forced.unlocks().next().unwrap().apiname, "ACH02");
    assert_eq!(
        fixture.read_store("tester")["42"]["items"][1]["achieved"],
        json!(true)
    );

    assert!(runtime.tick().skipped, "força é one-shot");
    assert!(!runtime.capture_gate().is_allowed());
}

#[test]
fn force_next_capture_releases_each_consumer_once_like_capturar_agora() {
    let fixture = Fixture::new("force-components");
    fixture.write_schema(
        "42",
        &[("1", "0", "ACH01", &[("brazilian", "Primeiros passos")])],
    );
    fixture.write_store(
        "tester",
        json!({"42": {"items": [item("ACH01", "Primeiros passos", 1, 0, false)]}}),
    );
    let mut runtime = fixture.runtime("tester", &["42"]);
    runtime.set_capture_gate(CaptureGate::blocked());
    runtime.force_next_capture();

    let schemas = runtime.catch_up();
    assert!(!schemas.skipped_progress, "schemas consome a própria força");
    let tick = runtime.tick();
    assert!(tick.forced, "vigia também consome a própria força");

    assert!(runtime.catch_up().skipped_progress, "schemas já consumiu");
    assert!(runtime.tick().skipped, "vigia já consumiu");
}

#[test]
fn catch_up_blocked_still_builds_schema_but_does_not_read_progress() {
    let fixture = Fixture::new("gate-catch-up");
    fixture.write_schema(
        "42",
        &[("1", "0", "ACH01", &[("brazilian", "Primeiros passos")])],
    );
    fixture.write_progress("42", &[("1", "0", 500)]);
    let mut runtime = fixture.runtime("tester", &["42"]);
    runtime.set_capture_gate(false);

    let blocked = runtime.catch_up();
    assert!(blocked.skipped_progress);
    assert_eq!(blocked.updated, 1, "schema é reconstruído mesmo pausado");
    let stored = fixture.read_store("tester");
    assert_eq!(stored["42"]["items"][0]["achieved"], json!(false));
    assert_eq!(stored["42"]["items"][0]["unlock"], json!(0));

    runtime.force_next_capture();
    let forced = runtime.catch_up();
    assert!(!forced.skipped_progress);
    assert!(forced.forced);
    let stored = fixture.read_store("tester");
    assert_eq!(stored["42"]["items"][0]["achieved"], json!(true));
    assert_eq!(stored["42"]["items"][0]["unlock"], json!(500));

    assert!(runtime.catch_up().skipped_progress, "volta à regra do gate");
}

#[test]
fn unlock_without_local_item_is_reported_as_unindexed() {
    let fixture = Fixture::new("unindexed");
    fixture.write_store("tester", json!({}));
    fixture.write_progress("42", &[("1", "0", 100)]);
    let mut runtime = fixture.runtime("tester", &["42"]);
    assert!(runtime.tick().events.is_empty(), "baseline");

    fixture.write_progress("42", &[("1", "0", 100), ("1", "1", 200)]);
    let report = runtime.tick();
    assert!(report.events.is_empty());
    assert_eq!(report.unindexed.len(), 1);
    assert_eq!(report.unindexed[0].appid, "42");
    assert_eq!(report.unindexed[0].key, "1|1");
    assert_eq!(report.unindexed[0].unlock, 200);
    assert!(report.unindexed[0].file.contains("UserGameStats_"));
    assert!(!report.changed);
}

#[test]
fn multiple_appids_emit_sorted_unlocks_with_per_game_counts() {
    let fixture = Fixture::new("multiple");
    fixture.write_store(
        "tester",
        json!({
            "7": {"items": [
                item("B1", "B um", 1, 0, false),
                item("B2", "B dois", 1, 1, false),
            ]},
            "42": {"items": [
                item("A1", "A um", 1, 0, false),
                item("A2", "A dois", 1, 1, false),
            ]},
        }),
    );
    fixture.write_progress("42", &[("1", "0", 100)]);
    fixture.write_progress("7", &[("1", "0", 100)]);
    let mut runtime = fixture.runtime("tester", &["42", "7"]);
    assert_eq!(runtime.tick().scanned_files, 2);

    fixture.write_progress("42", &[("1", "0", 100), ("1", "1", 200)]);
    fixture.write_progress("7", &[("1", "0", 100), ("1", "1", 700)]);
    let report = runtime.tick();
    assert_eq!(report.scanned_appids, 2);
    assert_eq!(report.scanned_files, 2);
    let events: Vec<_> = report.unlocks().collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].appid, "42");
    assert_eq!(events[0].apiname, "A2");
    assert_eq!(events[0].unlock, 200);
    assert_eq!((events[0].done, events[0].total), (1, 2));
    assert_eq!(events[1].appid, "7");
    assert_eq!(events[1].apiname, "B2");
    assert_eq!(events[1].unlock, 700);
    assert_eq!((events[1].done, events[1].total), (1, 2));
}

#[test]
fn zero_and_negative_progress_do_not_alert() {
    let fixture = Fixture::new("non-positive");
    fixture.write_store(
        "tester",
        json!({"42": {"items": [item("ACH01", "Primeiros passos", 1, 0, false)]}}),
    );
    fixture.write_progress("42", &[("1", "0", 100)]);
    let mut runtime = fixture.runtime("tester", &["42"]);
    assert!(runtime.tick().events.is_empty());

    fixture.write_progress("42", &[("1", "0", 100), ("1", "1", 0), ("1", "2", -5)]);
    let report = runtime.tick();
    assert!(report.events.is_empty());
    assert!(report.unindexed.is_empty(), "valor não positivo não alerta");
    assert!(!report.changed);
}

#[test]
fn corrupt_progress_bin_is_silent_and_recovers_when_rewritten() {
    let fixture = Fixture::new("corrupt-bin");
    fixture.write_store(
        "tester",
        json!({"42": {"items": [item("ACH01", "Primeiros passos", 1, 0, false)]}}),
    );
    fixture.write_progress_bytes("42", b"\x00\xffnot-a-kv-bin");
    let mut runtime = fixture.runtime("tester", &["42"]);
    assert!(runtime.tick().events.is_empty());
    assert!(runtime.tick().events.is_empty());

    fixture.write_progress("42", &[("1", "0", 123)]);
    let report = runtime.tick();
    assert_eq!(report.events.len(), 1);
    assert_eq!(report.unlocks().next().unwrap().unlock, 123);
    assert_eq!(
        fixture.read_store("tester")["42"]["items"][0]["achieved"],
        json!(true)
    );
}

#[test]
fn corrupt_schema_and_bin_make_catch_up_a_noop() {
    let fixture = Fixture::new("corrupt-schema");
    fs::write(schema_path(&fixture.stats, "42"), b"\x00broken").unwrap();
    fixture.write_progress_bytes("42", b"\x01broken");
    let mut runtime = fixture.runtime("tester", &["42"]);
    let report = runtime.catch_up();
    assert_eq!(report.updated, 0);
    assert_eq!(report.total, 0);
    assert!(report.errors.is_empty());
}

#[test]
fn persistence_round_trip_does_not_reemit_or_lose_local_state() {
    let fixture = Fixture::new("round-trip");
    fixture.write_store(
        "tester",
        json!({"42": {"items": [
            item("ACH01", "Primeiros passos", 1, 0, false),
            item("ACH02", "Segunda", 1, 1, false),
        ]}}),
    );
    fixture.write_progress("42", &[("1", "0", 100)]);
    {
        let mut runtime = fixture.runtime("tester", &["42"]);
        assert!(runtime.tick().events.is_empty());
        fixture.write_progress("42", &[("1", "0", 100), ("1", "1", 200)]);
        assert_eq!(runtime.tick().events.len(), 1);
    }

    let mut reopened = fixture.runtime("tester", &["42"]);
    assert!(
        reopened.tick().events.is_empty(),
        "estado persistido dedupa"
    );
    let catch = reopened.catch_up();
    assert!(!catch.skipped_progress);
    let stored = fixture.read_store("tester");
    assert_eq!(stored["42"]["items"][1]["achieved"], json!(true));
    assert_eq!(stored["42"]["items"][1]["unlock"], json!(200));
    assert_eq!(stored["42"]["items"][1]["localUnlock"], json!(true));
    assert_eq!(
        stored["42"]["items"][0]["achieved"],
        json!(true),
        "catch_up sincroniza o progresso antigo sem toast"
    );

    let leftovers = fs::read_dir(fixture.account_dir("tester"))
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
        .count();
    assert_eq!(leftovers, 0, "gravação atômica não deixa temporário");
}

#[test]
fn event_payload_carries_schema_name_description_icon_and_hidden() {
    let fixture = Fixture::new("payload");
    fixture.write_schema_hidden(
        "42",
        &[("1", "0", "ACH01", &[("brazilian", "Primeiros passos")])],
        &[("1", "0")],
    );
    fixture.write_store(
        "tester",
        json!({"42": {"items": [item("ACH01", "Primeiros passos", 1, 0, false)]}}),
    );
    let mut runtime = fixture.runtime("tester", &["42"]);
    assert!(runtime.tick().events.is_empty(), "sem bin no boot");

    fixture.write_progress("42", &[("1", "0", 1_700_000_000)]);
    let report = runtime.tick();
    let event = report.unlocks().next().unwrap();
    assert!(event.hidden, "hidden vem do schema do emulador/Steam");
    let value = event.to_value();
    assert_eq!(value["appid"], json!("42"));
    assert_eq!(value["key"], json!("1|0"));
    assert_eq!(value["apiname"], json!("ACH01"));
    assert_eq!(value["name"], json!("Primeiros passos"));
    assert_eq!(value["description"], json!("desc ACH01"));
    assert_eq!(value["icon"], json!("ACH01.jpg"));
    assert_eq!(value["hidden"], json!(true));
    assert_eq!(value["unlockTime"], json!(1_700_000_000i64));
    assert_eq!(value["unlock"], json!(1_700_000_000i64));
    assert_eq!(value["done"], json!(1));
    assert_eq!(value["total"], json!(1));
    assert_eq!(value["localUnlock"], json!(true));
    assert_eq!(value["provider"], json!("steam"));
}

#[test]
fn cracked_goldberg_save_path_unlocks_via_existing_watcher() {
    let fixture = Fixture::new("goldberg");
    fixture.write_store(
        "tester",
        json!({"999000": {"items": [
            item("ACH01", "Primeiros passos", 1, 0, false),
        ]}}),
    );
    let save_dir = fixture.appdata.join("GSE Saves").join("999000");
    fs::create_dir_all(&save_dir).unwrap();
    fs::write(
        save_dir.join("achievements.json"),
        json!({"ACH01": {"earned": true, "earned_time": 1_700_000_000}}).to_string(),
    )
    .unwrap();

    let mut runtime = fixture.runtime("tester", &["999000"]);
    runtime.set_platform(Platform::Windows);
    runtime.set_crack_environment(fixture.crack_environment());
    runtime.set_library(json!({"games": [{"id": "steam:999000", "installed": true}]}));

    let report = runtime.tick();
    assert_eq!(report.events.len(), 1);
    let event = report.unlocks().next().unwrap();
    assert_eq!(event.appid, "999000");
    assert_eq!(event.apiname, "ACH01");
    assert_eq!(event.provider, "goldberg");
    assert_eq!(event.unlock, 1_700_000_000);
    assert_eq!(event.title, "Primeiros passos");
    let stored = fixture.read_store("tester");
    assert_eq!(stored["999000"]["items"][0]["achieved"], json!(true));
    assert_eq!(stored["999000"]["items"][0]["unlock"], json!(1_700_000_000));
    assert_eq!(
        runtime.queue().get("999000", "ACH01").unwrap().unlocked_at,
        1_700_000_000
    );
    assert!(runtime.tick().events.is_empty(), "mtime não mudou");
}

#[derive(Default)]
struct MemoryFs {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeMap<PathBuf, Vec<String>>,
}

impl MemoryFs {
    fn file(&mut self, path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) {
        let path = path.into();
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let entries = self.dirs.entry(parent.to_path_buf()).or_default();
            let name = name.to_string_lossy().into_owned();
            if !entries.contains(&name) {
                entries.push(name);
            }
        }
        self.files.insert(path, bytes.into());
    }
}

impl WatcherFs for MemoryFs {
    fn exists(&self, path: &Path) -> bool {
        self.files.contains_key(path) || self.dirs.contains_key(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains_key(path)
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        String::from_utf8(self.files.get(path)?.clone()).ok()
    }

    fn read_bytes(&self, path: &Path) -> Option<Vec<u8>> {
        self.files.get(path).cloned()
    }

    fn read_dir(&self, path: &Path) -> Option<Vec<String>> {
        self.dirs.get(path).cloned()
    }

    fn modified_ms(&self, _path: &Path) -> Option<i64> {
        Some(1)
    }
}

struct FixedClock(i64);

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

#[test]
fn catch_up_and_tick_read_through_the_injected_filesystem() {
    let fixture = Fixture::new("memory-fs");
    let stats = fixture.stats.clone();
    let mut memory = MemoryFs::default();
    memory.file(
        schema_path(&stats, "42"),
        schema_bin(&[("1", "0", "ACH01", &[("brazilian", "Primeiros passos")])]),
    );
    memory.file(
        stats.join("UserGameStats_76561198000000000_42.bin"),
        progress_bin(&[("1", "0", 500)]),
    );

    let mut runtime = AchievementsRuntime::new(
        &fixture.data,
        "tester",
        &fixture.steam,
        ["42"],
        Arc::new(memory),
        Arc::new(FixedClock(1_700_000_000_000)),
    );
    let report = runtime.catch_up();
    assert_eq!(report.updated, 1);
    assert_eq!(report.total_appids, 1);
    let stored = fixture.read_store("tester");
    assert_eq!(stored["42"]["items"][0]["achieved"], json!(true));
    assert_eq!(stored["42"]["items"][0]["unlock"], json!(500));
    assert_eq!(
        runtime.load_store().items("42")[0]["achieved"],
        json!(true),
        "store lido do disco quando o FS virtual não o expõe"
    );
    assert!(
        runtime.tick().events.is_empty(),
        "o snapshot também veio do FS injetado"
    );
}

#[test]
fn injected_clock_sets_fallback_unlock_and_toast_window() {
    let fixture = Fixture::new("clock");
    fixture.write_store(
        "tester",
        json!({"999000": {"items": [
            item("ACH01", "Primeiros passos", 1, 0, false),
        ]}}),
    );
    let save_dir = fixture.appdata.join("GSE Saves").join("999000");
    fs::create_dir_all(&save_dir).unwrap();
    fs::write(
        save_dir.join("achievements.json"),
        json!({"ACH01": {"earned": true, "earned_time": 0}}).to_string(),
    )
    .unwrap();

    let now_ms = 1_700_000_000_123i64;
    let mut runtime = AchievementsRuntime::new(
        &fixture.data,
        "tester",
        &fixture.steam,
        ["999000"],
        Arc::new(SystemFs),
        Arc::new(FixedClock(now_ms)),
    );
    runtime.set_platform(Platform::Windows);
    runtime.set_crack_environment(fixture.crack_environment());
    runtime.set_library(json!({"games": [{"id": "steam:999000", "installed": true}]}));

    let report = runtime.tick();
    let event = report.unlocks().next().unwrap();
    assert_eq!(event.unlock, 1_700_000_000, "earned_time=0 usa o relógio");
    assert_eq!(runtime.now_seconds(), 1_700_000_000);
    assert!(runtime.should_toast(event));
    assert!(!runtime.should_toast(&UnlockEvent {
        unlock: 1_700_000_000 - TOAST_WINDOW_SECONDS - 1,
        ..event.clone()
    }));
}

#[test]
fn upc_runtime_revokes_local_unlock_and_clears_the_queue() {
    let fixture = Fixture::new("upc");
    fixture.write_store(
        "tester",
        json!({"3751950": {"items": [
            local_item("ACH01", "Primeira", 1, 0),
            local_item("ACH02", "Segunda", 1, 1),
        ]}}),
    );
    let upc_dir = fixture
        .appdata
        .join("Goldberg UplayEmu Saves")
        .join("66088");
    fs::create_dir_all(&upc_dir).unwrap();
    fs::write(
        upc_dir.join("achievements.json"),
        json!({
            "1001": {"earned": 1, "earned_time": 1_700_000_000, "displayName": "Primeira"},
            "1002": {"earned": 0, "earned_time": 0, "displayName": "Segunda"},
        })
        .to_string(),
    )
    .unwrap();

    let mut runtime = fixture.runtime("tester", &["3751950"]);
    runtime.set_platform(Platform::Windows);
    runtime.set_crack_environment(fixture.crack_environment());
    runtime.set_library(json!({"games": [{"id": "steam:3751950", "installed": true}]}));
    runtime.set_settings(json!({"steam:3751950": {"upcId": "66088"}}));
    runtime.set_allow_revoke(true);

    let mut queued = AchievementRecord::new("3751950", "ACH02");
    queued.achieved = true;
    queued.unlocked_at = 1_700_000_000;
    runtime.manager_mut().observe(queued);
    assert!(runtime.queue().get("3751950", "ACH02").is_some());

    let report = runtime.tick();
    assert_eq!(report.revocations().count(), 1);
    let event = report.revocations().next().unwrap();
    assert_eq!(event.appid, "3751950");
    assert_eq!(event.apiname, "ACH02");
    assert_eq!(event.key, "1|1");
    assert_eq!(event.title, "Segunda");
    assert_eq!(event.provider, "upc");
    assert_eq!(event.reason, "upc-runtime");
    assert_eq!(
        event.callback_value(),
        json!({"appid": "3751950", "apiname": "ACH02"})
    );

    let stored = fixture.read_store("tester");
    let items = stored["3751950"]["items"].as_array().unwrap();
    assert_eq!(items[0]["achieved"], json!(true), "earned=1 fica");
    assert_eq!(items[1]["achieved"], json!(false));
    assert_eq!(items[1]["unlock"], json!(0));
    assert_eq!(items[1]["revoked"], json!(true));
    assert!(runtime.queue().get("3751950", "ACH02").is_none());
    assert!(runtime.tick().events.is_empty(), "revogação não repete");
}

#[test]
fn revoke_requires_the_allow_revoke_flag_like_the_first_sync_pull() {
    let fixture = Fixture::new("upc-no-revoke");
    fixture.write_store(
        "tester",
        json!({"3751950": {"items": [local_item("ACH02", "Segunda", 1, 1)]}}),
    );
    let upc_dir = fixture
        .appdata
        .join("Goldberg UplayEmu Saves")
        .join("66088");
    fs::create_dir_all(&upc_dir).unwrap();
    fs::write(
        upc_dir.join("achievements.json"),
        json!({
            "1001": {"earned": 1, "earned_time": 1_700_000_000, "displayName": "Outra"},
            "1002": {"earned": 0, "earned_time": 0, "displayName": "Segunda"},
        })
        .to_string(),
    )
    .unwrap();

    let mut runtime = fixture.runtime("tester", &["3751950"]);
    runtime.set_platform(Platform::Windows);
    runtime.set_crack_environment(fixture.crack_environment());
    runtime.set_library(json!({"games": [{"id": "steam:3751950", "installed": true}]}));
    runtime.set_settings(json!({"steam:3751950": {"upcId": "66088"}}));

    let report = runtime.tick();
    assert!(report.revocations().next().is_none());
    assert_eq!(
        fixture.read_store("tester")["3751950"]["items"][0]["achieved"],
        json!(true)
    );
}

#[test]
fn reload_config_merges_appids_from_library_and_settings() {
    let fixture = Fixture::new("config");
    fs::write(
        fixture.data.join("library.json"),
        json!({"games": [{"id": "steam:10"}]}).to_string(),
    )
    .unwrap();
    let account_dir = fixture.account_dir("tester");
    fs::create_dir_all(&account_dir).unwrap();
    fs::write(
        account_dir.join("game_settings.json"),
        json!({"steam:11": {"exePath": "C:/game/game.exe"}}).to_string(),
    )
    .unwrap();

    let runtime = fixture.runtime("tester", &[]);
    let appids: Vec<&str> = runtime.appids().iter().map(String::as_str).collect();
    assert_eq!(appids, ["10", "11"]);
    assert_eq!(runtime.account(), "tester");
    assert_eq!(
        runtime.achievements_path(),
        fixture
            .data
            .join("contas")
            .join("tester")
            .join("achievements.json")
    );
    assert_eq!(runtime.stats_dir(), fixture.stats);
}
