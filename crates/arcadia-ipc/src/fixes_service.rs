//! Serviço de fixes do IPC: catálogo remoto, raízes seguras e engine.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use arcadia_fixes::catalog::FixCatalog;
use arcadia_fixes::install::{PathRoots, StdFixFs};
use arcadia_fixes::plan::FixEngine;

use crate::AppState;

pub fn roots(state: &AppState) -> PathRoots {
    let home = arcadia_store::paths::home_dir();
    let mut game_roots: Vec<PathBuf> = vec![
        state.data_dir.join("games"),
        home.join("Games").join("Arcadia"),
    ];
    for library in arcadia_steamstore::library::steam_libraries(
        &home,
        &arcadia_steamstore::library::SystemFs,
        None,
    ) {
        game_roots.push(library.path.join("steamapps").join("common"));
    }
    let mut prefix_roots: Vec<PathBuf> = vec![
        state.data_dir.join("prefixes"),
        home.join("Games").join("Arcadia").join("Prefixes"),
    ];
    if let Some(value) = arcadia_store::config::read_config(&state.data_dir)
        .get("default_wine_prefix_path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        prefix_roots.push(PathBuf::from(value));
    }
    PathRoots::new(
        game_roots,
        prefix_roots,
        state.data_dir.join("backups"),
        state.data_dir.clone(),
    )
}

pub fn engine(state: &AppState, catalog: FixCatalog) -> FixEngine {
    FixEngine::new(catalog, Arc::new(StdFixFs), roots(state))
}

pub fn empty_engine(state: &AppState) -> FixEngine {
    engine(state, FixCatalog::new())
}

async fn fetch_index(client: &arcadia_catalog::catalog::CatalogClient, path: &str) -> Option<Value> {
    client.get(path).await.data
}

pub async fn catalog(state: &AppState) -> Option<FixCatalog> {
    let client = crate::catalog_service::client(state)?;
    let fixes = fetch_index(&client, "/catalog/v1/fixes").await;
    let ryuu = fetch_index(&client, "/catalog/v1/ryuu").await;
    Some(FixCatalog::from_values(fixes.as_ref(), ryuu.as_ref()))
}

pub fn require_install_path(payload: &Value) -> Result<String, Value> {
    let path = payload
        .get("installPath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if path.is_empty() || path.contains("..") {
        return Err(json!({ "ok": false, "error": "installPath rejeitado" }));
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(json!({ "ok": false, "error": "installPath rejeitado" }));
    }
    Ok(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::require_install_path;
    use serde_json::json;

    #[test]
    fn rejects_relative_and_traversal_paths() {
        assert!(require_install_path(&json!({ "installPath": "../evil" })).is_err());
        assert!(require_install_path(&json!({ "installPath": "relative" })).is_err());
        #[cfg(windows)]
        assert!(require_install_path(&json!({ "installPath": r"C:\Games\Safe" })).is_ok());
    }
}
