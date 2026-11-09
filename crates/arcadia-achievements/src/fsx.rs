//! Injeção de filesystem/relógio para os vigias e inspetores.
//!
//! O cliente Electron usa `fs.*` e `Date.now()` diretamente. Aqui os módulos
//! puros recebem [`WatcherFs`] e [`Clock`]; testes usam implementações em
//! memória e nunca observam diretórios, dormem ou disparam timers.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Plataforma simulável (o watcher usa `process.platform` no JS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Windows,
    Unix,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }

    pub fn is_windows(self) -> bool {
        matches!(self, Self::Windows)
    }
}

/// Operações de filesystem que os decisores de watcher precisam.
pub trait WatcherFs {
    fn exists(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> Option<String>;
    /// Nomes dos filhos (só o nome, sem o diretório pai).
    fn read_dir(&self, path: &Path) -> Option<Vec<String>>;
    /// `mtimeMs` truncado em milissegundos.
    fn modified_ms(&self, path: &Path) -> Option<i64>;
}

/// Implementação real sobre `std::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFs;

impl WatcherFs for SystemFs {
    fn exists(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok()
    }

    fn is_file(&self, path: &Path) -> bool {
        fs::metadata(path)
            .map(|meta| meta.is_file())
            .unwrap_or(false)
    }

    fn is_dir(&self, path: &Path) -> bool {
        fs::metadata(path)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    fn read_dir(&self, path: &Path) -> Option<Vec<String>> {
        let mut names: Vec<String> = fs::read_dir(path)
            .ok()?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Some(names)
    }

    fn modified_ms(&self, path: &Path) -> Option<i64> {
        let modified = fs::metadata(path).ok()?.modified().ok()?;
        Some(millis_since_epoch(modified))
    }
}

fn millis_since_epoch(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// Relógio injetável (`Date.now()`).
pub trait Clock {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        millis_since_epoch(SystemTime::now())
    }
}

/// Raiz de dados de teste/consumidor, com o mesmo `join` do `path`.
pub fn join(base: impl AsRef<Path>, parts: &[&str]) -> PathBuf {
    let mut path = base.as_ref().to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_and_clock_match_the_current_environment() {
        assert_eq!(Platform::current().is_windows(), cfg!(windows));
        assert!(SystemClock.now_ms() > 0);
    }

    #[test]
    fn system_fs_reads_and_times_real_files() {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-fsx-{}-{}",
            std::process::id(),
            SystemClock.now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        fs::write(&file, "olá").unwrap();
        let fs = SystemFs;
        assert!(fs.exists(&dir));
        assert!(fs.is_dir(&dir));
        assert!(fs.is_file(&file));
        assert_eq!(fs.read_to_string(&file).as_deref(), Some("olá"));
        assert_eq!(fs.read_dir(&dir), Some(vec!["x.txt".to_string()]));
        assert!(fs.modified_ms(&file).unwrap() > 0);
        assert_eq!(fs.read_to_string(&dir.join("missing")), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
