//! Lista de Proton/Wine instalados pela Steam, no shape do seletor da UI.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use arcadia_steamstore::library::{steam_libraries, SystemFs};

use crate::AppState;

fn push_proton(
    root: &Path,
    name: &str,
    seen: &mut HashSet<String>,
    out: &mut Vec<Value>,
) {
    if seen.contains(name) {
        return;
    }
    let directory = root.join(name);
    let proton = directory.join("proton");
    let wine_bin = directory.join("files").join("bin").join("wine");
    let vdf = directory.join("compatibilitytool.vdf");
    if !(proton.exists() || wine_bin.exists() || vdf.exists()) {
        return;
    }
    seen.insert(name.to_string());
    out.push(json!({
        "id": format!("steam:{name}"),
        "name": format!("{name} (Steam)"),
        "path": directory.to_string_lossy(),
        "wine": if wine_bin.exists() {
            wine_bin.to_string_lossy().into_owned()
        } else {
            proton.to_string_lossy().into_owned()
        },
        "kind": "steam"
    }));
}

pub fn wine_list(_state: &AppState) -> Value {
    let home = arcadia_store::paths::home_dir();
    let libraries = steam_libraries(&home, &SystemFs, None);
    let mut seen = HashSet::new();
    let mut installed = Vec::new();

    for library in libraries {
        let common = library.path.join("steamapps").join("common");
        if let Ok(entries) = std::fs::read_dir(&common) {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.to_ascii_lowercase().starts_with("proton") {
                    continue;
                }
                push_proton(&common, &name, &mut seen, &mut installed);
            }
        }
        let compat = library.steam_dir.join("compatibilitytools.d");
        if let Ok(entries) = std::fs::read_dir(&compat) {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                push_proton(&compat, &name, &mut seen, &mut installed);
            }
        }
    }

    json!({ "installed": installed, "available": [] })
}

#[cfg(test)]
mod tests {
    use super::wine_list;
    use crate::AppState;

    #[test]
    fn wine_list_is_an_envelope() {
        let dir = std::env::temp_dir().join(format!("arcadia-wine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(dir.clone());
        let value = wine_list(&state);
        assert!(value["installed"].is_array());
        assert!(value["available"].is_array());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
