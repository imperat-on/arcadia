//! Utilitários de teste: diretório temporário real (removido no Drop) e chmod
//! de executável. Nenhum teste toca no home do usuário.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let base = std::env::temp_dir();
        for _ in 0..10_000 {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = base.join(format!("{prefix}-{}-{id}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("temporário: {error}"),
            }
        }
        panic!("não foi possível criar diretório temporário");
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, relative: &str) -> PathBuf {
        crate::fs::normalize_path(&self.path.join(relative))
    }

    pub fn abs(&self, relative: &str) -> String {
        crate::fs::path_string(&crate::fs::normalize_path(&self.path.join(relative)))
    }

    pub fn mkdir(&self, relative: &str) -> PathBuf {
        let path = self.join(relative);
        std::fs::create_dir_all(&path).expect("mkdir temporário");
        path
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir do arquivo temporário");
        }
        std::fs::write(&path, bytes).expect("arquivo temporário");
        path
    }

    pub fn read_dir_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.path)
            .expect("read_dir temporário")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Marca o arquivo como executável em Unix; no Windows o registry considera
/// qualquer arquivo regular executável.
pub fn chmod_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
    #[cfg(not(unix))]
    let _ = path;
}
