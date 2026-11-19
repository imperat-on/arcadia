use super::*;
use crate::test_support::FixtureFetcher;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

fn appinfo_json() -> Value {
    json!({
        "data": {
            "730": {
                "common": {"name": "Counter-Strike 2"},
                "config": {"installdir": "Counter-Strike Global Offensive"},
                "extended": {"listofdlc": "900, 901"},
                "depots": {
                    "branches": {
                        "public": {"buildid": "200", "description": "public", "timeupdated": 1700000000},
                        "beta": {"buildid": "201", "description": "beta", "pwdrequired": "1"}
                    },
                    "731": {
                        "name": "Game",
                        "config": {"oslist": "windows", "language": ""},
                        "manifests": {"public": {"gid": "111", "size": 1000, "download": 800}}
                    },
                    "732": {
                        "name": "Linux",
                        "config": {"oslist": "linux"},
                        "manifests": {"public": {"gid": "222", "size": 500}}
                    },
                    "733": {
                        "name": "Brazilian",
                        "config": {"oslist": "windows", "language": "brazilian"},
                        "manifests": {"public": {"gid": "333", "size": 100}}
                    },
                    "734": {
                        "name": "Shared",
                        "config": {},
                        "sharedinstall": "1",
                        "manifests": {"public": {"gid": "444", "size": 10}}
                    },
                    "735": {
                        "name": "DLC depot",
                        "config": {},
                        "dlcappid": "900",
                        "manifests": {"public": {"gid": "555", "size": 200}}
                    },
                    "736": {
                        "name": "Optional",
                        "config": {},
                        "optional": "1",
                        "manifests": {"public": {"gid": "666", "size": 300}}
                    }
                }
            }
        }
    })
}

fn dlc_appinfo() -> AppInfo {
    parse_app_info(
        &json!({"data": {"900": {
            "common": {"name": "DLC Name"},
            "depots": {
                "735": {
                    "name": "DLC depot",
                    "config": {"oslist": "windows", "language": "english"},
                    "manifests": {"public": {"gid": "777", "size": 200}}
                },
                "737": {"name": "Only DLC"}
            }
        }}}),
        "900",
    )
    .unwrap()
}

fn lua_depot(depot: &str, manifest: &str, key: &str, size: u64) -> LuaDepot {
    LuaDepot {
        depot_id: depot.to_string(),
        manifest_id: manifest.to_string(),
        key: key.to_string(),
        size,
    }
}

#[test]
fn sanitize_install_dir_matches_the_frozen_rules() {
    assert_eq!(sanitize_install_dir("Team Fortress 2"), "Team Fortress 2");
    assert_eq!(sanitize_install_dir("../../evil"), "evil");
    assert_eq!(sanitize_install_dir("C:\\Games\\X"), "CGamesX");
    assert_eq!(sanitize_install_dir("jogo:com/barra*?"), "jogocombarra");
    assert_eq!(sanitize_install_dir("   "), "jogo");
    assert_eq!(sanitize_install_dir(""), "jogo");
    assert_eq!(sanitize_install_dir("..."), ".");
    assert_eq!(sanitize_install_dir("a_b-c.d"), "a_b-c.d");
    assert_eq!(install_dir_default("Portal 2: Deluxe!"), "Portal2Deluxe");
    assert_eq!(install_dir_default(""), "");
}

#[test]
fn parse_lua_reads_keys_manifests_token_and_dlcs() {
    let text = r#"
addappid(730, 1, "abcd")
addappid(731, 1, "beef")
setManifestid(730, "111", 123)
setManifestid(731, "222")
addtoken("tok")
addappid(900)
addappid(730)
"#;
    let parsed = parse_lua_manifest(text, "730");
    assert_eq!(
        parsed.depots,
        vec![
            lua_depot("730", "111", "abcd", 123),
            lua_depot("731", "222", "beef", 0)
        ]
    );
    assert_eq!(parsed.token, "tok");
    assert_eq!(parsed.dlcs, vec!["900"]);
}

#[test]
fn parse_lua_files_merges_in_order_and_dedupes_dlcs() {
    let first = (
        "a.lua".to_string(),
        "setManifestid(1, \"10\")\naddtoken(\"one\")\naddappid(900)".to_string(),
    );
    let second = (
        "b.lua".to_string(),
        "addappid(2, 1, \"bbb\")\nsetManifestid(2, \"20\")\naddappid(900)\naddappid(901)\naddtoken(\"two\")"
            .to_string(),
    );
    let parsed = parse_lua_files(&[first, second], "1");
    assert_eq!(parsed.depots.len(), 2);
    assert_eq!(parsed.depots[1].depot_id, "2");
    assert_eq!(parsed.depots[1].key, "bbb");
    assert_eq!(parsed.token, "two");
    assert_eq!(parsed.dlcs, vec!["900", "901"]);
}

#[test]
fn parse_app_info_reads_depots_branches_manifests_and_metadata() {
    let info = parse_app_info(&appinfo_json(), "730").unwrap();
    assert_eq!(info.appid, "730");
    assert_eq!(info.name, "Counter-Strike 2");
    assert_eq!(info.installdir, "Counter-Strike Global Offensive");
    assert_eq!(info.list_of_dlc, vec!["900", "901"]);
    assert_eq!(info.depots.len(), 6);
    let game = info.depot("731").unwrap();
    assert_eq!(game.name.as_deref(), Some("Game"));
    assert_eq!(game.os.as_deref(), Some("windows"));
    assert_eq!(game.manifest_id, "111");
    assert_eq!(game.size, 1000);
    assert_eq!(game.manifest_for("public").unwrap().download, 800);
    assert_eq!(info.depot("734").unwrap().shared, Some(true));
    assert_eq!(info.depot("735").unwrap().dlc_appid.as_deref(), Some("900"));
    assert_eq!(info.depot("736").unwrap().optional, Some(true));
    assert_eq!(info.public_buildid(), Some("200"));
    assert!(info.branch("beta").unwrap().pwd_required);
}

#[test]
fn parse_app_info_accepts_the_app_object_without_the_data_wrapper() {
    let value = json!({
        "common": {"name": "X"},
        "config": {"installdir": "X"},
        "depots": {"1": {"manifests": {"public": {"gid": "9"}}}}
    });
    let info = parse_app_info(&value, "1").unwrap();
    assert_eq!(info.name, "X");
    assert_eq!(info.depots[0].manifest_id, "9");
    assert!(parse_app_info(&json!({}), "1").is_none());
}

#[test]
fn enrich_depots_uses_game_and_dlc_pics_without_losing_lua_ids() {
    let game = parse_app_info(&appinfo_json(), "730").unwrap();
    let dlc = dlc_appinfo();
    let mut depots = vec![
        DepotInfo::from_lua(&lua_depot("731", "111", "k1", 1000)),
        DepotInfo::from_lua(&lua_depot("735", "555", "k2", 200)),
        DepotInfo::from_lua(&lua_depot("999", "999", "", 1)),
    ];
    enrich_depots(&mut depots, &game, &[dlc]);
    assert_eq!(depots[0].os.as_deref(), Some("windows"));
    assert_eq!(depots[0].name.as_deref(), Some("Game"));
    assert_eq!(depots[0].manifest_id, "111");
    assert_eq!(depots[1].dlc_appid.as_deref(), Some("900"));
    assert_eq!(depots[1].name.as_deref(), Some("DLC Name"));
    assert_eq!(depots[1].os.as_deref(), Some("windows"));
    assert_eq!(depots[1].language.as_deref(), Some("english"));
    // Sem PICS: campos continuam ausentes, mas o depot do .lua permanece.
    assert_eq!(depots[2].name, None);
    assert_eq!(depots[2].os, None);
}

#[test]
fn enrich_depots_uses_game_name_when_the_depot_has_no_name() {
    let mut game = parse_app_info(&appinfo_json(), "730").unwrap();
    game.name = "Game Fallback".to_string();
    game.depot_mut("731").unwrap().name = Some(String::new());
    let mut depots = vec![DepotInfo::from_lua(&lua_depot("731", "111", "", 0))];
    enrich_depots(&mut depots, &game, &[]);
    assert_eq!(depots[0].name.as_deref(), Some("Game Fallback"));
}

#[test]
fn merge_early_exits_when_the_leader_has_every_manifest() {
    let leader = ProviderManifest {
        nome: "Hubcap".to_string(),
        depots: vec![
            lua_depot("730", "1", "a", 10),
            lua_depot("731", "2", "b", 20),
        ],
        token: "tok".to_string(),
        dlcs: vec!["900".to_string()],
        manifests_present: ["730", "731"].iter().map(|id| id.to_string()).collect(),
    };
    let other = ProviderManifest {
        nome: "Ryuu".to_string(),
        depots: vec![lua_depot("999", "3", "c", 30)],
        manifests_present: ["999"].iter().map(|id| id.to_string()).collect(),
        ..ProviderManifest::default()
    };
    let merged = merge_provider_manifests(&[leader, other]);
    assert_eq!(merged.depots.len(), 2);
    assert_eq!(merged.fonte, "Hubcap");
    assert_eq!(merged.token, "tok");
    assert_eq!(merged.dlcs, vec!["900"]);
    assert_eq!(merged.contribuidores, vec!["Hubcap"]);
    assert!(merged.skipped.is_empty());
}

#[test]
fn merge_fills_missing_manifests_from_other_providers() {
    let leader = ProviderManifest {
        nome: "Hubcap".to_string(),
        depots: vec![
            lua_depot("730", "1", "a", 10),
            lua_depot("731", "2", "b", 20),
        ],
        token: "".to_string(),
        dlcs: vec![],
        manifests_present: ["730"].iter().map(|id| id.to_string()).collect(),
    };
    let other = ProviderManifest {
        nome: "Ryuu".to_string(),
        depots: vec![lua_depot("731", "22", "bee", 200)],
        token: "tok2".to_string(),
        dlcs: vec!["901".to_string()],
        manifests_present: ["731"].iter().map(|id| id.to_string()).collect(),
    };
    let merged = merge_provider_manifests(&[leader, other]);
    assert_eq!(merged.depots.len(), 2);
    assert_eq!(merged.depots[0].manifest_id, "1");
    assert_eq!(merged.depots[1].manifest_id, "22");
    assert_eq!(merged.depots[1].key, "bee");
    assert_eq!(merged.token, "tok2");
    assert_eq!(merged.dlcs, vec!["901"]);
    assert_eq!(merged.fonte, "Hubcap (+manifests de Ryuu)");
    assert_eq!(merged.contribuidores, vec!["Hubcap", "Ryuu"]);
}

#[test]
fn merge_skips_depots_without_any_local_manifest() {
    let provider = ProviderManifest {
        nome: "Hubcap".to_string(),
        depots: vec![lua_depot("1", "10", "", 0), lua_depot("2", "20", "", 0)],
        manifests_present: ["1"].iter().map(|id| id.to_string()).collect(),
        ..ProviderManifest::default()
    };
    let merged = merge_provider_manifests(&[provider]);
    assert_eq!(merged.depots.len(), 1);
    assert_eq!(merged.skipped, vec!["2"]);
    assert_eq!(merged.fonte, "Hubcap");
}

#[test]
fn install_info_envelope_matches_store_install_info_shape() {
    let game = parse_app_info(&appinfo_json(), "730").unwrap();
    let provider = ProviderManifest {
        nome: "Hubcap".to_string(),
        depots: vec![lua_depot("731", "111", "k", 1000)],
        token: "tok".to_string(),
        dlcs: vec!["900".to_string()],
        manifests_present: ["731"].iter().map(|id| id.to_string()).collect(),
    };
    let merged = merge_provider_manifests(&[provider]);
    let response = install_info_from_merged("730", merged, Some(&game), &[dlc_appinfo()]);
    assert!(response.ok);
    assert_eq!(response.appid.as_deref(), Some("730"));
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["appid"], "730");
    assert_eq!(value["depots"][0]["depotId"], "731");
    assert_eq!(value["depots"][0]["manifestId"], "111");
    assert_eq!(value["depots"][0]["key"], "k");
    assert_eq!(value["depots"][0]["size"], 1000);
    assert_eq!(value["token"], "tok");
    assert_eq!(value["dlcs"][0], "900");
    assert_eq!(value["fonte"], "Hubcap");
    assert_eq!(value["installdir"], "Counter-Strike Global Offensive");
}

#[test]
fn install_info_without_depots_is_the_frozen_error() {
    let response = install_info_from_merged("730", MergedManifest::default(), None, &[]);
    assert!(!response.ok);
    assert_eq!(
        response.error.as_deref(),
        Some("nenhum provedor disponível")
    );
}

#[test]
fn depot_picker_groups_base_languages_and_dlcs_in_order() {
    let depots = vec![
        depot("730", Some("windows"), None, None, Some("Game"), false),
        depot("731", Some("windows"), Some("brazilian"), None, None, false),
        depot("732", Some("windows"), Some("english"), None, None, false),
        depot(
            "733",
            Some("windows"),
            None,
            Some("900"),
            Some("DLC Name"),
            false,
        ),
        depot("734", Some("windows"), None, None, None, true),
        depot("735", Some("macos"), None, None, None, false),
        depot("736", None, None, None, None, false),
    ];
    let groups = depot_groups(&depots);
    assert_eq!(groups.len(), 4);
    assert_eq!(groups[0].tipo, DepotGroupKind::Base);
    assert_eq!(groups[0].titulo, "Base");
    assert_eq!(groups[0].depots.len(), 1);
    assert_eq!(groups[1].tipo, DepotGroupKind::Idioma);
    assert_eq!(groups[1].titulo, "Brazilian");
    assert_eq!(groups[1].depots[0].depot_id, "731");
    assert_eq!(groups[2].tipo, DepotGroupKind::Idioma);
    assert_eq!(groups[2].titulo, "English");
    assert_eq!(groups[3].tipo, DepotGroupKind::Dlc);
    assert_eq!(groups[3].titulo, "DLC Name");
    assert!(groups.iter().all(|group| !group
        .depots
        .iter()
        .any(|depot| matches!(depot.depot_id.as_str(), "734" | "735" | "736"))));
}

#[test]
fn depot_picker_without_any_metadata_returns_a_single_sem_meta_group() {
    let depots = vec![lua_info("1", "10"), lua_info("2", "20")];
    let groups = depot_groups(&depots);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, "sem_meta");
    assert_eq!(groups[0].tipo, DepotGroupKind::SemMeta);
    assert_eq!(groups[0].depots.len(), 2);
}

#[test]
fn picker_default_selection_prefers_brazilian_then_english_and_windows() {
    let depots = vec![
        depot("730", Some("windows"), None, None, Some("Game"), false),
        depot("731", Some("windows"), Some("brazilian"), None, None, false),
        depot("732", Some("windows"), Some("english"), None, None, false),
        depot("733", Some("linux"), None, None, None, false),
        depot("734", None, None, None, None, false),
        depot(
            "735",
            Some("windows"),
            None,
            Some("900"),
            Some("DLC"),
            false,
        ),
    ];
    assert_eq!(preferred_language(&depots, "pt-BR"), "brazilian");
    assert_eq!(preferred_language(&depots[..1], "pt-BR"), "english");
    let essential = essential_selection(&depots);
    assert_eq!(
        essential.into_iter().collect::<Vec<_>>(),
        vec!["730".to_string(), "734".to_string()]
    );
    let selected = default_selection(&depots, "pt-BR");
    assert_eq!(
        selected.clone().into_iter().collect::<Vec<_>>(),
        vec!["730".to_string(), "731".to_string(), "734".to_string()]
    );
    assert_eq!(selected_size(&depots, &selected), 0);
    assert_eq!(size_gib(1024_u64.pow(3)), 1.0);
}

#[test]
fn size_math_sums_selected_depots_and_saturates() {
    let depots = vec![
        DepotInfo {
            size: 1_000,
            ..lua_info("1", "10")
        },
        DepotInfo {
            size: 2_000,
            ..lua_info("2", "20")
        },
        DepotInfo {
            size: u64::MAX,
            ..lua_info("3", "30")
        },
    ];
    let selection = BTreeSet::from(["1".to_string(), "2".to_string()]);
    assert_eq!(selected_size(&depots, &selection), 3_000);
    let all = BTreeSet::from(["1".to_string(), "2".to_string(), "3".to_string()]);
    assert_eq!(selected_size(&depots, &all), u64::MAX);
    assert_eq!(size_gib(2 * 1024_u64.pow(3)), 2.0);
}

#[test]
fn branch_manifests_override_lua_manifest_and_size() {
    let app = parse_app_info(
        &json!({"data": {"730": {
            "common": {"name": "X"},
            "depots": {
                "1": {"manifests": {
                    "public": {"gid": "100", "size": 1000},
                    "beta": {"gid": "200", "size": 2000}
                }},
                "2": {"manifests": {"public": {"gid": "300", "size": 3000}}}
            }
        }}}),
        "730",
    )
    .unwrap();
    let mut depots = vec![
        DepotInfo {
            manifest_id: "lua1".to_string(),
            size: 1,
            ..lua_info("1", "lua1")
        },
        DepotInfo {
            manifest_id: "lua2".to_string(),
            size: 2,
            ..lua_info("2", "lua2")
        },
        DepotInfo {
            manifest_id: "lua3".to_string(),
            size: 3,
            ..lua_info("3", "lua3")
        },
    ];
    assert_eq!(apply_branch_manifests(&mut depots, &app, "public"), 2);
    assert_eq!(depots[0].manifest_id, "100");
    assert_eq!(depots[0].size, 1000);
    assert_eq!(depots[1].manifest_id, "300");
    assert_eq!(depots[2].manifest_id, "lua3");
    assert_eq!(apply_branch_manifests(&mut depots, &app, "beta"), 1);
    assert_eq!(depots[0].manifest_id, "200");
    assert_eq!(depots[0].size, 2000);
    assert_eq!(depots[1].manifest_id, "300");
}

#[test]
fn depot_picker_json_exposes_groups_and_sizes() {
    let depots = vec![
        DepotInfo {
            size: 1024,
            ..lua_info("1", "10")
        },
        DepotInfo {
            size: 1024,
            ..lua_info("2", "20")
        },
    ];
    let value = depot_picker_json(&depots, "en-US");
    assert_eq!(value["grupos"][0]["tipo"], "sem_meta");
    assert_eq!(value["total_bytes"], 2048);
    assert_eq!(value["idioma"], "english");
}

fn depot(
    id: &str,
    os: Option<&str>,
    language: Option<&str>,
    dlc: Option<&str>,
    name: Option<&str>,
    shared: bool,
) -> DepotInfo {
    DepotInfo {
        depot_id: id.to_string(),
        manifest_id: format!("m{id}"),
        key: String::new(),
        size: 0,
        name: name.map(str::to_string),
        os: os.map(str::to_string),
        language: language.map(str::to_string),
        dlc_appid: dlc.map(str::to_string),
        shared: Some(shared),
        optional: None,
        manifests: BTreeMap::new(),
    }
}

fn lua_info(id: &str, manifest: &str) -> DepotInfo {
    DepotInfo::from_lua(&lua_depot(id, manifest, "", 0))
}

#[tokio::test]
async fn depot_client_fetches_app_info_name_and_latest_buildid() {
    let fetcher =
        Arc::new(FixtureFetcher::new().json("api.steamcmd.net/v1/info/730", appinfo_json()));
    let client = DepotClient::new(fetcher.clone());
    let info = client.app_info("730").await.unwrap();
    assert_eq!(info.name, "Counter-Strike 2");
    assert_eq!(
        client.app_name("730").await.as_deref(),
        Some("Counter-Strike 2")
    );
    assert_eq!(
        client.latest_buildid("730", "public").await.as_deref(),
        Some("200")
    );
    assert_eq!(
        client.latest_buildid("730", "beta").await.as_deref(),
        Some("201")
    );
    assert_eq!(fetcher.urls()[0], "https://api.steamcmd.net/v1/info/730");
}

#[tokio::test]
async fn depot_client_install_info_enriches_and_returns_installdir() {
    let fetcher =
        Arc::new(FixtureFetcher::new().json("api.steamcmd.net/v1/info/730", appinfo_json()));
    let client = DepotClient::new(fetcher);
    let provider = ProviderManifest {
        nome: "Hubcap".to_string(),
        depots: vec![lua_depot("731", "111", "k", 1000)],
        token: "tok".to_string(),
        dlcs: vec![],
        manifests_present: ["731"].iter().map(|id| id.to_string()).collect(),
    };
    let response = client.install_info("730", &[provider]).await;
    assert!(response.ok);
    assert_eq!(
        response.installdir.as_deref(),
        Some("Counter-Strike Global Offensive")
    );
    assert_eq!(response.depots.unwrap()[0].os.as_deref(), Some("windows"));
}

#[tokio::test]
async fn depot_client_install_info_enriches_dlc_names_from_pics() {
    let fetcher = Arc::new(
        FixtureFetcher::new()
            .json("api.steamcmd.net/v1/info/730", appinfo_json())
            .json(
                "api.steamcmd.net/v1/info/900",
                json!({"data": {"900": {
                    "common": {"name": "DLC Name"},
                    "depots": {"735": {"config": {"oslist": "windows"}}}
                }}}),
            ),
    );
    let client = DepotClient::new(fetcher);
    let provider = ProviderManifest {
        nome: "Hubcap".to_string(),
        depots: vec![lua_depot("735", "555", "k", 200)],
        manifests_present: ["735"].iter().map(|id| id.to_string()).collect(),
        ..ProviderManifest::default()
    };
    let response = client.install_info("730", &[provider]).await;
    let depots = response.depots.unwrap();
    assert_eq!(depots[0].dlc_appid.as_deref(), Some("900"));
    assert_eq!(depots[0].name.as_deref(), Some("DLC Name"));
}

#[tokio::test]
async fn depot_client_failure_is_none_not_a_panic() {
    let fetcher = Arc::new(FixtureFetcher::new().text("api.steamcmd.net", 404, ""));
    let client = DepotClient::new(fetcher.clone());
    assert!(client.app_info("1").await.is_none());
    assert!(client.app_name("1").await.is_none());
    assert!(client.app_info("../1").await.is_none());
    assert_eq!(fetcher.calls().len(), 2);
}
