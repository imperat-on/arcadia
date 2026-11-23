//! Descoberta local dos componentes históricos, sem carregar nem executar arquivos.
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Default)]
pub(crate) struct DetectedPlugins {
    pub slssteam: bool,
    pub luatools: bool,
}

pub(crate) fn detect(data_dir: &Path) -> DetectedPlugins {
    let home = arcadia_store::paths::home_dir();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let mut roots = arcadia_steamstore::library::steam_base_candidates(&home);
    roots.push(PathBuf::from(crate::steam_service::steam_root()));
    detect_at(
        &home,
        &data_home,
        &roots,
        &arcadia_store::config::read_config(data_dir),
    )
}

fn detect_at(
    home: &Path,
    data_home: &Path,
    steam_roots: &[PathBuf],
    config: &Value,
) -> DetectedPlugins {
    let manual = config
        .get("slssteam_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let slssteam = if let Some(path) = manual {
        let path = if path == "~" {
            home.to_path_buf()
        } else if let Some(rest) = path.strip_prefix("~/") {
            home.join(rest)
        } else {
            PathBuf::from(path)
        };
        path.is_file() || path.join("SLSsteam.so").is_file()
    } else {
        [
            data_home.join("SLSsteam/SLSsteam.so"),
            home.join(".local/share/SLSsteam/SLSsteam.so"),
        ]
        .iter()
        .any(|path| path.is_file())
            || steam_roots
                .iter()
                .any(|root| root.join("SLSsteam.so").is_file())
    };
    let luatools = steam_roots
        .iter()
        .any(|root| root.join("steamui/webkit/luatools.js").is_file())
        || data_home
            .join("Lumen/luatools/public/luatools.js")
            .is_file();
    DetectedPlugins { slssteam, luatools }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_files_not_empty_folders_and_honors_manual_override() {
        let root =
            std::env::temp_dir().join(format!("arcadia-plugin-detect-{}", std::process::id()));
        let home = root.join("home");
        let data = root.join("xdg");
        let steam = root.join("steam");
        std::fs::create_dir_all(data.join("SLSsteam")).unwrap();
        std::fs::create_dir_all(steam.join("steamui/webkit")).unwrap();
        let roots = vec![steam.clone()];
        let missing = detect_at(&home, &data, &roots, &json!({}));
        assert!(!missing.slssteam && !missing.luatools);
        std::fs::write(data.join("SLSsteam/SLSsteam.so"), b"fixture").unwrap();
        std::fs::write(steam.join("steamui/webkit/luatools.js"), b"fixture").unwrap();
        let found = detect_at(&home, &data, &roots, &json!({}));
        assert!(found.slssteam && found.luatools);
        let overridden = detect_at(
            &home,
            &data,
            &roots,
            &json!({"slssteam_path":root.join("missing.so")}),
        );
        assert!(!overridden.slssteam && overridden.luatools);
        let manual = root.join("custom.so");
        std::fs::write(&manual, b"fixture").unwrap();
        assert!(detect_at(&home, &data, &roots, &json!({"slssteam_path":manual})).slssteam);
        std::fs::remove_file(data.join("SLSsteam/SLSsteam.so")).unwrap();
        assert!(!detect_at(&home, &data, &roots, &json!({})).slssteam);
        let _ = std::fs::remove_dir_all(root);
    }
}
