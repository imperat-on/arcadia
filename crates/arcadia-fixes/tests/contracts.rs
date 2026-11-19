//! Contrato público do crate (usado pela borda IPC/Tauri).

use std::path::PathBuf;
use std::sync::Arc;

use arcadia_fixes::{
    single_root_roots, ApplyRequest, ArchiveEntry, ArchiveLayout, FixCatalog, FixEngine, FixStatus,
    FixType, PathRoots, StdFixFs,
};
use serde_json::json;

fn setup(temp: &tempfile::TempDir) -> (FixEngine, PathBuf) {
    let game = temp.path().join("game");
    std::fs::create_dir_all(&game).unwrap();
    let catalog = FixCatalog::from_values(
        Some(&json!({"genericFixes": [730], "onlineFixes": [730]})),
        Some(&json!({"fixes": {"730": [{"file": "bypass.zip", "badge": "bypass"}]}})),
    );
    let roots = single_root_roots(&game, temp.path().join("data"));
    let engine = FixEngine::new(catalog, Arc::new(StdFixFs), roots);
    (engine, game)
}

#[test]
fn public_engine_contract_end_to_end() {
    let temp = tempfile::tempdir().unwrap();
    let (engine, game) = setup(&temp);

    let check = engine.check("steam:730");
    assert!(check.ok);
    assert_eq!(check.appid.as_deref(), Some("730"));
    assert_eq!(check.auth_configured, Some(false));

    let request = ApplyRequest::new(
        "730",
        FixType::Generic,
        "https://files.luatools.work/GameBypasses/730.zip",
        game.to_string_lossy().to_string(),
    );
    let layout = ArchiveLayout::new(vec![
        ArchiveEntry::file("hook.dll", b"hook".to_vec()),
        ArchiveEntry::file("launcher.exe", b"exe".to_vec()),
    ]);
    let response = engine.apply_archive(&request, &layout);
    assert_eq!(
        serde_json::to_value(&response).unwrap(),
        json!({"ok": true})
    );
    assert_eq!(
        serde_json::to_value(engine.installed("730", Some(&game.to_string_lossy()))).unwrap(),
        json!({"ok": true, "installed": true})
    );
    assert!(engine.status("730").is_terminal());
    assert_eq!(engine.status("730"), FixStatus::done());

    let redirect = engine.launcher_redirect(Some(&game.to_string_lossy()));
    assert_eq!(
        redirect.redirect,
        Some(Some(format!(
            "\"{}\" %command%",
            game.join("launcher.exe").to_string_lossy()
        )))
    );
    assert_eq!(
        serde_json::to_value(engine.unfix("730", Some(&game.to_string_lossy()))).unwrap(),
        json!({"ok": true})
    );
    assert!(!game.join("hook.dll").exists());
}

#[test]
fn public_catalog_and_roots_contract() {
    let catalog = FixCatalog::from_values(
        Some(&json!({"genericFixes": [1, "2"]})),
        Some(&json!([{"appid": 3, "name": "Three", "fixes": [
            {"file": "three.zip", "badges": ["crack", "bypass"]}
        ]}])),
    );
    assert_eq!(catalog.check_generic("2").status, 200);
    assert_eq!(catalog.check_online("1").status, 404);
    let crack = catalog.check_crack("3");
    assert_eq!(crack.game_name.as_deref(), Some("Three"));
    assert_eq!(crack.badge.as_deref(), Some("bypass"));

    let temp = tempfile::tempdir().unwrap();
    let roots: PathRoots = single_root_roots(temp.path().join("game"), temp.path().join("data"));
    assert!(roots
        .validate_install_path(temp.path().join("game").to_str().unwrap())
        .is_ok());
    assert!(roots
        .validate_install_path(temp.path().join("outside").to_str().unwrap())
        .is_err());
}
