//! Resolução de caminhos — espelho de `app/electron/runtime-paths.js`.

use std::path::{Path, PathBuf};

/// `HOME` (Unix) ou `USERPROFILE` (Windows).
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

fn resolve_from(
    override_value: Option<&str>,
    platform_windows: bool,
    home: &Path,
    local_app_data: Option<&Path>,
) -> PathBuf {
    if let Some(value) = override_value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            return if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(&path))
                    .unwrap_or(path)
            };
        }
    }
    if platform_windows {
        local_app_data
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"))
            .join("arcadia")
    } else {
        home.join(".local").join("share").join("arcadia")
    }
}

/// Raiz de dados efetiva: `ARCADIA_DATA_DIR` vence, senão o default da
/// plataforma. Idêntico ao cliente Electron.
pub fn resolve_data_dir() -> PathBuf {
    let override_value = std::env::var("ARCADIA_DATA_DIR").ok();
    resolve_from(
        override_value.as_deref(),
        cfg!(windows),
        &home_dir(),
        local_app_data().as_deref(),
    )
}

pub fn data_path(parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(resolve_data_dir(), |acc, part| acc.join(part))
}

/// `dataDir/contas/<user>/...` quando há conta; raiz quando guest.
pub fn account_path(user: Option<&str>, parts: &[&str]) -> PathBuf {
    let mut base = resolve_data_dir();
    if let Some(user) = user.map(str::trim).filter(|u| !u.is_empty()) {
        base = base.join("contas").join(user);
    }
    parts.iter().fold(base, |acc, part| acc.join(part))
}

pub fn platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_from;
    use std::path::Path;

    #[test]
    fn default_windows_is_local_app_data() {
        let out = resolve_from(
            None,
            true,
            Path::new(r"C:\Users\zes"),
            Some(Path::new(r"C:\Users\zes\AppData\Local")),
        );
        assert_eq!(out, Path::new(r"C:\Users\zes\AppData\Local").join("arcadia"));
    }

    #[test]
    fn default_linux_is_local_share() {
        let out = resolve_from(None, false, Path::new("/home/zes"), None);
        assert_eq!(out, Path::new("/home/zes/.local/share/arcadia"));
    }

    #[test]
    fn override_wins_and_trims() {
        // `/tmp` é absoluto no Unix; no Windows é drive-relative e o resolver
        // junta com o cwd. A fixture usa um caminho absoluto de cada plataforma.
        let (raw, expected) = if cfg!(windows) {
            (r"  C:\tmp\arcadia-test  ", r"C:\tmp\arcadia-test")
        } else {
            ("  /tmp/arcadia-test  ", "/tmp/arcadia-test")
        };
        let out = resolve_from(Some(raw), cfg!(windows), Path::new("/home/zes"), None);
        assert_eq!(out, Path::new(expected));
    }

    #[test]
    fn blank_override_falls_back() {
        let out = resolve_from(Some("   "), false, Path::new("/home/zes"), None);
        assert_eq!(out, Path::new("/home/zes/.local/share/arcadia"));
    }
}
