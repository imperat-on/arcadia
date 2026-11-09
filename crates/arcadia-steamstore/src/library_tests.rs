use super::*;
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn library(path: &Path) -> SteamLibrary {
    SteamLibrary {
        path: path.to_path_buf(),
        steam_dir: path.parent().unwrap().to_path_buf(),
        free: 0,
    }
}

#[test]
fn parse_library_folders_preserves_order_and_dedupes() {
    let text = r#"
"libraryfolders"
{
    "0"
    {
        "path"        "C:\\Steam"
        "label"        ""
        "apps" { "730" "123" }
    }
    "1"
    {
        "path"        "/mnt/games/SteamLibrary"
    }
    "2"
    {
        "path"        "C:\\Steam"
    }
}
"#;
    let folders = parse_library_folders(text);
    assert_eq!(
        folders,
        vec![
            LibraryFolder {
                path: PathBuf::from("C:/Steam")
            },
            LibraryFolder {
                path: PathBuf::from("/mnt/games/SteamLibrary")
            }
        ]
    );
}

#[test]
fn parse_library_folders_falls_back_to_regex_on_malformed_vdf() {
    let folders = parse_library_folders(r#""path" "D:\\Games\\Steam""#);
    assert_eq!(
        folders,
        vec![LibraryFolder {
            path: PathBuf::from("D:/Games/Steam")
        }]
    );
}

#[test]
fn discover_libraries_reads_each_root_and_applies_free_space() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("Steam");
    let library_root = temp.path().join("Library");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    write(
        &root.join("steamapps").join("libraryfolders.vdf"),
        &format!(
            "\"libraryfolders\"\n{{\n \"0\" {{ \"path\" \"{}\" }}\n}}\n",
            library_root.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    let fs_impl = SystemFs;
    let libraries = discover_libraries(
        &[root.clone(), temp.path().join("missing")],
        &fs_impl,
        Some(&|_| 1234),
    );
    assert_eq!(libraries.len(), 1);
    assert_eq!(libraries[0].path, library_root.join("steamapps"));
    assert_eq!(libraries[0].steam_dir, library_root);
    assert_eq!(libraries[0].free, 1234);
}

#[test]
fn discover_libraries_falls_back_to_the_first_root_steamapps() {
    let temp = TempDir::new().unwrap();
    let fs_impl = SystemFs;
    let root = temp.path().join("Steam");
    let libraries = discover_libraries(std::slice::from_ref(&root), &fs_impl, None);
    assert_eq!(libraries.len(), 1);
    assert_eq!(libraries[0].path, root.join("steamapps"));
    assert_eq!(libraries[0].steam_dir, root);
    assert_eq!(libraries[0].free, 0);
    assert!(discover_libraries(&[], &fs_impl, None).is_empty());
}

#[test]
fn steam_libraries_uses_the_three_frozen_linux_roots() {
    let temp = TempDir::new().unwrap();
    let home = temp.path();
    let root = home.join(".steam").join("steam");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    write(
        &root.join("steamapps").join("libraryfolders.vdf"),
        "\"libraryfolders\"\n{\n \"0\" { \"path\" \"/mnt/lib\" }\n}\n",
    );
    let libraries = steam_libraries(home, &SystemFs, None);
    assert_eq!(libraries.len(), 1);
    assert_eq!(libraries[0].path, PathBuf::from("/mnt/lib/steamapps"));
}

#[test]
fn steam_apps_dirs_adds_main_and_each_library_without_duplicates() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("Steam");
    let extra = temp.path().join("Extra");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    write(
        &root.join("steamapps").join("libraryfolders.vdf"),
        &format!(
            "\"libraryfolders\"\n{{\n \"0\" {{ \"path\" \"{}\" }}\n \"1\" {{ \"path\" \"{}\" }}\n}}\n",
            root.to_string_lossy().replace('\\', "\\\\"),
            extra.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    let dirs = steam_apps_dirs(std::slice::from_ref(&root), &SystemFs);
    assert_eq!(dirs, vec![root.join("steamapps"), extra.join("steamapps")]);
}

#[test]
fn parse_acf_reads_installed_fields_and_the_arcadia_marker() {
    let text = r#"
"AppState"
{
    "appid"        "730"
    "Universe"     "1"
    "name"         "Counter-Strike 2"
    "StateFlags"   "4"
    "installdir"   "Counter-Strike Global Offensive"
    "LastUpdated"  "1700000000"
    "LastPlayed"   "1600000000"
    "SizeOnDisk"   "123456"
    "buildid"      "987654"
    "ArcadiaDownload" "1"
}
"#;
    let manifest = parse_acf(text).unwrap();
    assert_eq!(manifest.appid, "730");
    assert_eq!(manifest.name, "Counter-Strike 2");
    assert_eq!(manifest.installdir, "Counter-Strike Global Offensive");
    assert_eq!(manifest.state_flags, 4);
    assert_eq!(manifest.buildid, "987654");
    assert_eq!(manifest.size_on_disk, 123456);
    assert_eq!(manifest.last_updated, 1_700_000_000);
    assert_eq!(manifest.last_played, 1_600_000_000);
    assert!(manifest.arcadia_download);
    assert!(manifest.is_installed());

    let owned = parse_acf("\"AppState\" { \"appid\" \"10\" \"StateFlags\" \"4\" }").unwrap();
    assert!(!owned.arcadia_download);
    assert_eq!(owned.appid, "10");
    assert!(parse_acf("").is_none());
}

#[test]
fn write_acf_matches_the_frozen_minimum_and_sanitizes_install_dir() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("Steam");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    let path = write_acf_at(
        "730",
        "Counter-Strike \"2\"",
        "../../evil",
        &root,
        1_700_000_000,
        &SystemFs,
    )
    .unwrap();
    assert_eq!(path, root.join("steamapps").join("appmanifest_730.acf"));
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"Universe\"\t\t\"1\"\n\t\"name\"\t\t\"Counter-Strike \\\"2\\\"\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"evil\"\n\t\"LastUpdated\"\t\t\"1700000000\"\n\t\"LastPlayed\"\t\t\"0\"\n\t\"SizeOnDisk\"\t\t\"1\"\n\t\"buildid\"\t\t\"1\"\n\t\"ArcadiaDownload\"\t\t\"1\"\n}\n"
    );
    let parsed = parse_acf(&text).unwrap();
    assert_eq!(parsed.installdir, "evil");
    assert!(parsed.arcadia_download);
}

#[test]
fn scan_manifests_and_arcadia_downloaded_cover_every_library() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("First").join("steamapps");
    let second = temp.path().join("Second").join("steamapps");
    write(
        &first.join("appmanifest_100.acf"),
        "\"AppState\" { \"appid\" \"100\" \"name\" \"Marked\" \"StateFlags\" \"4\" \"installdir\" \"Marked\" \"ArcadiaDownload\" \"1\" }",
    );
    write(
        &first.join("appmanifest_200.acf"),
        "\"AppState\" { \"appid\" \"200\" \"name\" \"Owned\" \"StateFlags\" \"4\" \"installdir\" \"Owned\" }",
    );
    write(
        &second.join("appmanifest_300.acf"),
        "\"AppState\" { \"appid\" \"300\" \"name\" \"Other\" \"StateFlags\" \"2\" \"installdir\" \"Other\" \"ArcadiaDownload\" \"1\" }",
    );
    write(&first.join("not-an-acf.txt"), "x");
    let libraries = vec![library(&first), library(&second)];
    let downloaded = arcadia_downloaded(&libraries, &SystemFs);
    assert_eq!(downloaded.len(), 2);
    assert_eq!(downloaded[0].appid, "100");
    assert_eq!(downloaded[0].installdir, "Marked");
    assert_eq!(downloaded[1].appid, "300");
    let installed = installed_manifests(&libraries, &SystemFs);
    assert_eq!(
        installed
            .iter()
            .map(|manifest| manifest.appid.as_str())
            .collect::<Vec<_>>(),
        vec!["100", "200"]
    );
}

#[test]
fn game_install_dir_handles_steam_custom_and_epic() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("Steam");
    let extra = temp.path().join("Library");
    fs::create_dir_all(extra.join("steamapps").join("common")).unwrap();
    write(
        &root.join("steamapps").join("libraryfolders.vdf"),
        &format!(
            "\"libraryfolders\"\n{{\n \"0\" {{ \"path\" \"{}\" }}\n}}\n",
            extra.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    write(
        &extra.join("steamapps").join("appmanifest_730.acf"),
        "\"AppState\" { \"appid\" \"730\" \"StateFlags\" \"4\" \"installdir\" \"CS2\" }",
    );
    let roots = vec![root.clone()];
    let steam_game = GameRef::new("steam:730", "steam");
    assert_eq!(
        game_install_dir(&steam_game, &roots, None, &SystemFs),
        extra
            .join("steamapps")
            .join("common")
            .join("CS2")
            .to_string_lossy()
    );
    let custom = GameRef::new("custom:x", "custom").with_exe("/games/Best/game.exe");
    assert_eq!(
        game_install_dir(&custom, &roots, None, &SystemFs),
        Path::new("/games/Best").to_string_lossy()
    );

    let legendary = temp.path().join("legendary");
    write(
        &legendary.join("installed.json"),
        &json!({"AppId": {"install_path": "/games/Epic"}}).to_string(),
    );
    let epic = GameRef::new("epic:AppId", "epic");
    assert_eq!(
        game_install_dir(&epic, &roots, Some(&legendary), &SystemFs),
        "/games/Epic"
    );
    assert_eq!(
        game_install_dir(&GameRef::new("steam:999", "steam"), &roots, None, &SystemFs),
        ""
    );
    assert_eq!(
        game_install_dir_or(
            &GameRef::new("steam:999", "steam"),
            &roots,
            None,
            Some("/settings/game.exe"),
            &SystemFs
        ),
        Path::new("/settings").to_string_lossy()
    );
}

#[test]
fn remove_downloaded_deletes_only_marked_acfs_and_safe_install_dirs() {
    let temp = TempDir::new().unwrap();
    let steamapps = temp.path().join("Steam").join("steamapps");
    let common = steamapps.join("common");
    write(&common.join("Arcadia Game").join("game.exe"), "game");
    write(&temp.path().join("outside").join("keep.txt"), "keep");
    write(
        &steamapps.join("appmanifest_100.acf"),
        "\"AppState\" { \"appid\" \"100\" \"StateFlags\" \"4\" \"installdir\" \"Arcadia Game\" \"ArcadiaDownload\" \"1\" }",
    );
    write(
        &steamapps.join("appmanifest_101.acf"),
        "\"AppState\" { \"appid\" \"101\" \"StateFlags\" \"4\" \"installdir\" \"../outside\" \"ArcadiaDownload\" \"1\" }",
    );
    write(
        &steamapps.join("appmanifest_200.acf"),
        "\"AppState\" { \"appid\" \"200\" \"StateFlags\" \"4\" \"installdir\" \"Owned Game\" }",
    );
    let home = temp.path().join("home");
    let libraries = vec![library(&steamapps)];
    let result = remove_downloaded("100", &libraries, &home, None, &SystemFs);
    assert!(result.ok);
    assert_eq!(result.removidos, Some(1));
    assert!(!steamapps.join("appmanifest_100.acf").exists());
    assert!(!common.join("Arcadia Game").exists());
    assert!(steamapps.join("appmanifest_200.acf").exists());

    let result = remove_downloaded("101", &libraries, &home, None, &SystemFs);
    assert_eq!(result.removidos, Some(1));
    assert!(!steamapps.join("appmanifest_101.acf").exists());
    assert!(temp.path().join("outside").join("keep.txt").exists());
    assert!(steamapps.join("appmanifest_200.acf").exists());
}

#[test]
fn remove_downloaded_is_idempotent_for_unknown_app() {
    let temp = TempDir::new().unwrap();
    let steamapps = temp.path().join("Steam").join("steamapps");
    fs::create_dir_all(&steamapps).unwrap();
    let result = remove_downloaded(
        "999",
        &[library(&steamapps)],
        &temp.path().join("home"),
        None,
        &SystemFs,
    );
    assert!(result.ok);
    assert_eq!(result.removidos, Some(0));
}

#[test]
fn add_to_steam_comments_setmanifestid_and_copies_manifests() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let base = temp.path().join("Steam");
    let manifest_dir = temp.path().join("manifest_730");
    write(
        &manifest_dir.join("730.lua"),
        "addappid(730)\n  setManifestid(730, \"111\")\n",
    );
    write(&manifest_dir.join("730_111.manifest"), "manifest");
    let legacy_lua = home
        .join(".config")
        .join("SLSsteam")
        .join("config")
        .join("stplug-in")
        .join("730.lua");
    let legacy_manifest = home
        .join(".config")
        .join("SLSsteam")
        .join("manifests")
        .join("730_111.manifest");
    write(&legacy_lua, "addappid(730)\nsetManifestid(730, \"111\")");
    write(&legacy_manifest, "old");

    let result = add_to_steam("730", &manifest_dir, Some(&base), &home, &SystemFs);
    assert_eq!(
        result,
        AddToSteamResponse {
            ok: true,
            luas: Some(1),
            error: None
        }
    );
    let stplug = base.join("config").join("stplug-in");
    let lua = fs::read_to_string(stplug.join("730.lua")).unwrap();
    assert!(lua.contains("-- setManifestid(730, \"111\")"), "{lua}");
    assert!(!lua.contains("\nsetManifestid"));
    assert_eq!(
        fs::read_to_string(base.join("depotcache").join("730_111.manifest")).unwrap(),
        "manifest"
    );
    assert!(!legacy_lua.exists());
    assert!(!legacy_manifest.exists());
}

#[test]
fn add_to_steam_without_lua_or_steam_returns_the_frozen_errors() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let empty = temp.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    let result = add_to_steam(
        "730",
        &empty,
        Some(&temp.path().join("Steam")),
        &home,
        &SystemFs,
    );
    assert!(!result.ok);
    assert_eq!(
        result.error.as_deref(),
        Some("nenhum .lua no manifesto — baixe o manifesto antes (botão Baixar)")
    );
    let result = add_to_steam("730", &empty, None, &home, &SystemFs);
    assert_eq!(
        result.error.as_deref(),
        Some("instalação da Steam não encontrada")
    );
    let invalid = add_to_steam(
        "../../730",
        &empty,
        Some(&temp.path().join("Steam")),
        &home,
        &SystemFs,
    );
    assert_eq!(invalid.error.as_deref(), Some("appid inválido"));
}

#[test]
fn remove_from_steam_cleans_stplug_depotcache_and_yaml_sections() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let base = temp.path().join("Steam");
    let stplug = base.join("config").join("stplug-in");
    write(&stplug.join("730.lua"), "x");
    write(&stplug.join("730_extra.lua"), "x");
    write(&stplug.join("731.lua"), "x");
    write(&base.join("depotcache").join("730_111.manifest"), "m");
    write(&base.join("depotcache").join("731_222.manifest"), "m");
    let manifest_dir = temp.path().join("manifest_730");
    write(&manifest_dir.join("730_111.manifest"), "m");
    let config = home.join(".config").join("SLSsteam").join("config.yaml");
    write(
        &config,
        &[
            "AdditionalApps:",
            "  - 730",
            "  - 731",
            "AppTokens:",
            "  730: tok",
            "  731: tok2",
            "DlcData:",
            "  730:",
            "    900: \"DLC\"",
            "  731:",
            "    901: \"DLC\"",
        ]
        .join("\n"),
    );
    let result = remove_from_steam("730", Some(&base), &home, Some(&manifest_dir), &SystemFs);
    assert!(result.ok);
    assert!(!stplug.join("730.lua").exists());
    assert!(!stplug.join("730_extra.lua").exists());
    assert!(stplug.join("731.lua").exists());
    assert!(!base.join("depotcache").join("730_111.manifest").exists());
    assert!(base.join("depotcache").join("731_222.manifest").exists());
    let text = fs::read_to_string(&config).unwrap();
    assert!(!text.contains("730"));
    assert!(text.contains("731"));
    assert!(!text.contains("900"));
    assert!(text.contains("901"));
}

#[test]
fn register_slssteam_handles_inline_blocks_tokens_and_dlc_maps() {
    let yaml = "AdditionalApps: [123]\nAppTokens: {}\n";
    let first = register_slssteam(yaml, "456", "tok", &["900".to_string()]);
    assert!(first.contains("AdditionalApps: [123, 456]"), "{first}");
    assert!(first.contains("AppTokens:\n  456: tok"), "{first}");
    assert!(
        first.contains("DlcData:\n  456:\n    900: \"DLC\""),
        "{first}"
    );

    let second = register_slssteam(&first, "456", "tok", &["900".to_string()]);
    assert_eq!(second, first);
    assert_eq!(second.matches("456").count(), first.matches("456").count());

    let block = register_slssteam("AdditionalApps:\n  - 1\n", "2", "", &[]);
    assert!(block.contains("  - 1"), "{block}");
    assert!(block.contains("  - 2"), "{block}");
    assert!(!block.contains("AppTokens"));

    let with_dlc = register_slssteam(
        "DlcData:\n  456:\nAppTokens:\n  456: x\n",
        "456",
        "",
        &["901".to_string()],
    );
    assert!(
        with_dlc.contains("DlcData:\n  456:\n    901: \"DLC\""),
        "{with_dlc}"
    );
}

#[test]
fn remove_app_from_config_does_not_match_a_longer_appid_or_orphan_children() {
    let yaml = [
        "AdditionalApps:",
        "  - 4740",
        "  - 2114740",
        "AppTokens:",
        "  4740: tok",
        "  2114740: tok2",
        "DlcData:",
        "  4740:",
        "    999: \"DLC\"",
        "  2114740:",
        "    888: \"DLC\"",
    ]
    .join("\n");
    let updated = remove_app_from_config(&yaml, "4740");
    assert!(
        !updated.lines().any(|line| line.trim() == "- 4740"),
        "{updated}"
    );
    assert!(updated.contains("- 2114740"));
    assert!(!updated.contains("  4740:"));
    assert!(!updated.contains("999: \"DLC\""));
    assert!(updated.contains("2114740: tok2"));
    assert!(updated.contains("888: \"DLC\""));

    let inline = remove_app_from_config("AdditionalApps: [1, 2, 3]", "2");
    assert_eq!(inline, "AdditionalApps: [1, 3]");
    assert_eq!(
        remove_app_from_config("AdditionalApps: [2]", "2"),
        "AdditionalApps: []"
    );
}

#[test]
fn appids_injected_reads_additional_apps_and_stplug_names() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let base = temp.path().join("Steam");
    write(
        &home
            .join(".config")
            .join("SLSsteam")
            .join("config")
            .join("stplug-in")
            .join("123.lua"),
        "x",
    );
    write(
        &base.join("config").join("stplug-in").join("456_extra.lua"),
        "x",
    );
    write(&base.join("config").join("stplug-in").join("bad.lua"), "x");
    let yaml = "AdditionalApps:\n  - 123\n  - 999\nAppTokens:\n  888: tok\n";
    let ids = appids_injected_from(Some(yaml), Some(&base), &home, &SystemFs);
    assert_eq!(
        ids.into_iter().collect::<Vec<_>>(),
        vec!["123", "456", "999"]
    );
    write(
        &home.join(".config").join("SLSsteam").join("config.yaml"),
        yaml,
    );
    let ids = appids_injected(Some(&base), &home, &SystemFs);
    assert!(!ids.contains("888"));
    assert!(ids.contains("999"));
}

#[test]
fn find_steam_base_prefers_a_directory_with_steamapps_or_config() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir_all(second.join("steamapps")).unwrap();
    assert_eq!(
        find_steam_base(&[first, second.clone()], &SystemFs),
        Some(second)
    );
    assert_eq!(
        find_steam_base(&[temp.path().join("none")], &SystemFs),
        None
    );
    assert_eq!(
        steam_base_candidates(Path::new("/home/u")),
        vec![
            PathBuf::from("/home/u/.steam/steam"),
            PathBuf::from("/home/u/.local/share/Steam"),
            PathBuf::from("/home/u/.steam/debian-installation"),
            PathBuf::from("/home/u/.var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ]
    );
}

#[test]
fn pending_games_file_round_trips_through_the_injected_fs() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("pending_games.json");
    let games = vec![pending_stub("730", "CS2")];
    write_pending_games(&path, &games, &SystemFs).unwrap();
    let read = read_pending_games(&path, &SystemFs);
    assert_eq!(read, games);
    assert!(fs::read_to_string(&path).unwrap().contains("\n  {"));
    assert!(read_pending_games(&temp.path().join("missing.json"), &SystemFs).is_empty());
    write(&path, "not json");
    assert!(read_pending_games(&path, &SystemFs).is_empty());
}

#[test]
fn store_status_merges_injected_apps_and_arcadia_downloads() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let base = temp.path().join("Steam");
    let steamapps = base.join("steamapps");
    write(
        &home
            .join(".config")
            .join("SLSsteam")
            .join("config")
            .join("stplug-in")
            .join("123.lua"),
        "x",
    );
    write(
        &steamapps.join("appmanifest_456.acf"),
        "\"AppState\" { \"appid\" \"456\" \"StateFlags\" \"4\" \"installdir\" \"X\" \"ArcadiaDownload\" \"1\" }",
    );
    let status = store_status(
        StoreStatusInputs {
            dotnet: Some("dotnet".to_string()),
            depotdownloader: true,
            hubcap_key: true,
            slssteam: false,
            steam_dir: base.to_string_lossy().into_owned(),
        },
        &[library(&steamapps)],
        &home,
        Some(&base),
        &SystemFs,
    );
    assert_eq!(status.dotnet.as_deref(), Some("dotnet"));
    assert!(status.depotdownloader);
    assert!(status.hubcap_key);
    assert_eq!(status.adicionados, vec!["123", "456"]);
    let value = serde_json::to_value(&status).unwrap();
    assert_eq!(value["hubcapKey"], true);
    assert_eq!(value["steamDir"], base.to_string_lossy().as_ref());
}

#[test]
fn pending_stub_add_remove_and_mark_installed_are_pure_and_idempotent() {
    let added = add_pending_stub(&[], false, "730", "  Counter-Strike 2  ", None, None);
    assert!(added.changed);
    assert_eq!(added.games[0]["id"], "steam:730");
    assert_eq!(added.games[0]["title"], "Counter-Strike 2");
    assert_eq!(added.games[0]["launcher"], "steam");
    assert_eq!(added.games[0]["installed"], false);
    assert_eq!(added.games[0]["pendente"], true);
    assert_eq!(
        added.games[0]["cover"],
        "https://cdn.cloudflare.steamstatic.com/steam/apps/730/library_600x900.jpg"
    );

    let again = add_pending_stub(&added.games, false, "730", "X", None, None);
    assert!(!again.changed);
    let in_library = add_pending_stub(&[], true, "730", "X", None, None);
    assert!(!in_library.changed);
    let with_art = add_pending_stub(&[], false, "730", "", Some("c"), Some("h"));
    assert_eq!(with_art.games[0]["title"], "Steam 730");
    assert_eq!(with_art.games[0]["cover"], "c");
    assert_eq!(with_art.games[0]["hero"], "h");

    let marked = mark_steam_installed(&added.games, "730", "CS2", None, None);
    assert!(marked.changed);
    assert_eq!(marked.games[0]["installed"], true);
    assert_eq!(marked.games[0]["title"], "CS2");
    let created = mark_steam_installed(&[], "999", "", None, None);
    assert_eq!(created.games[0]["id"], "steam:999");
    assert_eq!(created.games[0]["installed"], true);
    let invalid = mark_steam_installed(&[], "abc", "X", None, None);
    assert!(!invalid.changed);

    let removed = remove_pending_stub(&marked.games, "730");
    assert!(removed.changed);
    assert!(removed.games.is_empty());
    let absent = remove_pending_stub(&removed.games, "730");
    assert!(!absent.changed);
}
