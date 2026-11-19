//! Utilitários de teste compartilhados.
//!
//! As conversões JavaScript usadas em produção vivem em [`crate::js`]; este
//! módulo só existe para os testes que precisam de um diretório temporário
//! (nenhuma instalação da Steam é necessária).

/// Diretório temporário com limpeza no `Drop`.
#[cfg(test)]
pub(crate) struct TestDir {
    path: std::path::PathBuf,
}

#[cfg(test)]
impl TestDir {
    pub(crate) fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "arcadia-steam-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("criar diretório temporário");
        Self { path }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
impl AsRef<std::path::Path> for TestDir {
    fn as_ref(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
